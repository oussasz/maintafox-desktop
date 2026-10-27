//! Data integrity: each detector on a seeded inconsistency (and silence on clean data), stable
//! finding identities across runs, waivers and repairs with optimistic locking and audit rows.

use sea_orm::DatabaseConnection;
use serde_json::Value;

use crate::data_integrity::queries::{get_finding, DataIntegrityFindingRow};
use crate::data_integrity::{
    apply_data_integrity_repair, list_open_findings, run_data_integrity_detectors, waive_data_integrity_finding,
    ApplyDataIntegrityRepairInput, WaiveDataIntegrityFindingInput,
};
use crate::errors::AppError;
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_db};

// ── Fixture helpers ─────────────────────────────────────────────────────────────────────────

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'", vec![]).await
}

async fn insert_user(db: &DatabaseConnection, username: &str) -> i64 {
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
         force_password_change, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', 1, 0, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![
            format!("di-user-{username}").into(),
            username.into(),
            username.into(),
        ],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await
}

/// `user` holds `integrity.repair` plant-wide, through a custom role assigned at tenant scope.
async fn grant_integrity_repair(db: &DatabaseConnection, user: i64) {
    let role = format!("di-repairers-{user}");
    exec(
        db,
        "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version)          VALUES (?, ?, 'custom', 'active', 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'),          strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)",
        vec![format!("{role}-sync").into(), role.clone().into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO role_permissions (role_id, permission_id, granted_at)          SELECT r.id, p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM roles r, permissions p           WHERE r.name = ? AND p.name = ?",
        vec![role.clone().into(), crate::rbac::permissions::INTEGRITY_REPAIR.into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,          created_at, updated_at, row_version)          SELECT ?, ?, id, 'tenant', NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'),          strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1 FROM roles WHERE name = ?",
        vec![format!("{role}-assignment").into(), user.into(), role.into()],
    )
    .await;
}

async fn insert_wo(db: &DatabaseConnection, code: &str, actual: Option<(&str, &str)>) -> i64 {
    let (start, end) = match actual {
        Some((s, e)) => (Some(s.to_string()), Some(e.to_string())),
        None => (None, None),
    };
    exec(
        db,
        "INSERT INTO work_orders (code, type_id, status_id, title, actual_start, actual_end, row_version, \
         created_at, updated_at) \
         VALUES (?, (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'), \
         (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), ?, ?, ?, 1, datetime('now'), datetime('now'))",
        vec![code.into(), code.into(), start.into(), end.into()],
    )
    .await;
    scalar_i64(db, "SELECT id FROM work_orders WHERE code = ?", vec![code.into()]).await
}

async fn insert_segment(db: &DatabaseConnection, wo: i64, started: &str, ended: Option<&str>) -> i64 {
    exec(
        db,
        "INSERT INTO work_order_downtime_segments (work_order_id, started_at, ended_at, downtime_type) \
         VALUES (?, ?, ?, 'full')",
        vec![wo.into(), started.into(), ended.map(str::to_string).into()],
    )
    .await;
    scalar_i64(db, "SELECT MAX(id) FROM work_order_downtime_segments", vec![]).await
}

async fn insert_failure_code(db: &DatabaseConnection, code: &str, code_type: &str, active: bool) -> i64 {
    exec(
        db,
        "INSERT INTO failure_codes (entity_sync_id, hierarchy_id, parent_id, code, label, code_type, is_active, \
         row_version) VALUES (?, (SELECT id FROM failure_hierarchies ORDER BY id LIMIT 1), NULL, ?, ?, ?, ?, 1)",
        vec![
            format!("fc:di-test:{code}").into(),
            code.into(),
            code.into(),
            code_type.into(),
            i64::from(active).into(),
        ],
    )
    .await;
    scalar_i64(db, "SELECT id FROM failure_codes WHERE code = ?", vec![code.into()]).await
}

async fn insert_failure_detail(db: &DatabaseConnection, wo: i64, mode: Option<i64>) -> i64 {
    exec(
        db,
        "INSERT INTO work_order_failure_details (work_order_id, failure_mode_id) VALUES (?, ?)",
        vec![wo.into(), mode.into()],
    )
    .await;
    scalar_i64(db, "SELECT MAX(id) FROM work_order_failure_details", vec![]).await
}

/// A value of the governed failure-mode list (the published WORK.FAILURE_MODES set).
async fn governed_mode(db: &DatabaseConnection, code: &str) -> i64 {
    let set_id = scalar_i64(
        db,
        "SELECT rs.id FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.FAILURE_MODES' AND rs.status = 'published' ORDER BY rs.id LIMIT 1",
        vec![],
    )
    .await;
    exec(
        db,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) VALUES (?, ?, ?, 1, 1)",
        vec![set_id.into(), code.into(), code.into()],
    )
    .await;
    scalar_i64(db, "SELECT id FROM reference_values WHERE code = ?", vec![code.into()]).await
}

/// A failure detail whose mode points nowhere (only possible with foreign keys off, e.g. an
/// import or a damaged database): the one case the repair may clear.
async fn dangling_failure_detail(db: &DatabaseConnection, wo: i64) -> (i64, i64) {
    let dangling = 987_654_i64;
    exec(db, "PRAGMA foreign_keys = OFF", vec![]).await;
    exec(db, "DELETE FROM reference_values WHERE id = ?", vec![dangling.into()]).await;
    exec(db, "DELETE FROM failure_codes WHERE id = ?", vec![dangling.into()]).await;
    let fd = insert_failure_detail(db, wo, Some(dangling)).await;
    exec(db, "PRAGMA foreign_keys = ON", vec![]).await;
    (fd, dangling)
}

/// Failure detail written without foreign-key checks. Since migration 178 the mode points to
/// `reference_values`; a legacy `failure_codes` id can only exist from older data or an old
/// device's sync, which is exactly what the detector must flag.
async fn insert_failure_detail_unchecked(db: &DatabaseConnection, wo: i64, mode: i64) -> i64 {
    exec(db, "PRAGMA foreign_keys = OFF", vec![]).await;
    let fd = insert_failure_detail(db, wo, Some(mode)).await;
    exec(db, "PRAGMA foreign_keys = ON", vec![]).await;
    fd
}

async fn segment_times(db: &DatabaseConnection, id: i64) -> (Option<String>, Option<String>) {
    (
        scalar_opt_text(
            db,
            "SELECT started_at FROM work_order_downtime_segments WHERE id = ?",
            vec![id.into()],
        )
        .await,
        scalar_opt_text(
            db,
            "SELECT ended_at FROM work_order_downtime_segments WHERE id = ?",
            vec![id.into()],
        )
        .await,
    )
}

async fn open_findings(db: &DatabaseConnection) -> Vec<DataIntegrityFindingRow> {
    list_open_findings(db, 500).await.expect("list open findings")
}

async fn only_open_finding(db: &DatabaseConnection, code: &str) -> DataIntegrityFindingRow {
    let all = open_findings(db).await;
    let matching: Vec<_> = all.iter().filter(|f| f.finding_code == code).cloned().collect();
    assert_eq!(matching.len(), 1, "one open {code}, have {all:?}");
    matching.into_iter().next().expect("finding")
}

fn details(f: &DataIntegrityFindingRow) -> Value {
    serde_json::from_str(&f.details_json).expect("details_json")
}

async fn audit_rows(db: &DatabaseConnection, finding_id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM data_integrity_repair_actions WHERE finding_id = ?",
        vec![finding_id.into()],
    )
    .await
}

/// (action, actor_id, before_json, after_json) of the single audit row of a finding.
async fn audit_row(db: &DatabaseConnection, finding_id: i64) -> (String, i64, Value, Value) {
    assert_eq!(audit_rows(db, finding_id).await, 1, "exactly one audit row");
    let text = |col: &'static str| async move {
        scalar_opt_text(
            db,
            &format!("SELECT {col} FROM data_integrity_repair_actions WHERE finding_id = ?"),
            vec![finding_id.into()],
        )
        .await
        .unwrap_or_else(|| panic!("{col} missing"))
    };
    let action = text("action").await;
    let before: Value = serde_json::from_str(&text("before_json").await).expect("before_json");
    let after: Value = serde_json::from_str(&text("after_json").await).expect("after_json");
    let actor = scalar_i64(
        db,
        "SELECT actor_id FROM data_integrity_repair_actions WHERE finding_id = ?",
        vec![finding_id.into()],
    )
    .await;
    (action, actor, before, after)
}

fn waive(f: &DataIntegrityFindingRow, rv: i64, reason: &str, approver: Option<i64>) -> WaiveDataIntegrityFindingInput {
    WaiveDataIntegrityFindingInput {
        finding_id: f.id,
        expected_row_version: rv,
        reason: reason.into(),
        approver_id: approver,
    }
}

fn repair(f: &DataIntegrityFindingRow, rv: i64, kind: &str) -> ApplyDataIntegrityRepairInput {
    ApplyDataIntegrityRepairInput {
        finding_id: f.id,
        expected_row_version: rv,
        repair_kind: kind.into(),
    }
}

fn validation_text(r: Result<DataIntegrityFindingRow, AppError>) -> String {
    match r {
        Err(AppError::ValidationFailed(m)) => m.join(" | "),
        other => panic!("expected ValidationFailed, got {other:?}"),
    }
}

// ── Detectors ───────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn clean_data_produces_no_findings() {
    let db = started_db().await;
    let wo = insert_wo(&db, "WO-CLEAN", Some(("2026-04-10T08:00:00Z", "2026-04-10T18:00:00Z"))).await;
    insert_segment(&db, wo, "2026-04-10T08:00:00Z", Some("2026-04-10T10:00:00Z")).await;
    insert_segment(&db, wo, "2026-04-10T11:00:00Z", Some("2026-04-10T13:00:00Z")).await;
    let mode = governed_mode(&db, "DI_OK_MODE").await;
    insert_failure_detail_unchecked(&db, wo, mode).await;
    insert_failure_detail(&db, wo, None).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 0);
    assert!(open_findings(&db).await.is_empty());
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM data_integrity_findings", vec![]).await,
        0
    );
}

#[tokio::test]
async fn failure_mode_outside_the_governed_list_is_flagged() {
    let db = started_db().await;
    let wo = insert_wo(&db, "WO-FM", None).await;
    let governed = governed_mode(&db, "DI_GOVERNED_MODE").await;
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    let legacy = insert_failure_code(&db, "DI_LEGACY_MODE", "mode", true).await;
    exec(&db, "DELETE FROM reference_values WHERE id = ?", vec![legacy.into()]).await;
    exec(&db, "PRAGMA foreign_keys = ON", vec![]).await;
    insert_failure_detail_unchecked(&db, wo, governed).await;
    let fd_legacy = insert_failure_detail_unchecked(&db, wo, legacy).await;
    let (fd_dangling, dangling) = dangling_failure_detail(&db, wo).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 2);
    let mut found: Vec<(i64, i64)> = open_findings(&db)
        .await
        .iter()
        .map(|f| {
            assert_eq!(f.finding_code, "FK_ORPHAN_FAILURE_MODE");
            assert_eq!(
                (f.severity.as_str(), f.domain.as_str(), f.record_class.as_str()),
                ("warning", "failure_coding", "wo_failure_details")
            );
            assert_eq!((f.status.as_str(), f.row_version), ("open", 1));
            let d = details(f);
            assert_eq!(d["work_order_id"].as_i64(), Some(wo));
            (f.record_id, d["failure_mode_id"].as_i64().expect("mode id"))
        })
        .collect();
    found.sort_unstable();
    let mut expected = vec![(fd_legacy, legacy), (fd_dangling, dangling)];
    expected.sort_unstable();
    assert_eq!(
        found, expected,
        "the governed value is valid; the legacy code and the dangling id are not"
    );
}

#[tokio::test]
async fn negative_downtime_segment_is_an_error_finding() {
    let db = started_db().await;
    let wo = insert_wo(&db, "WO-NEG", None).await;
    let seg = insert_segment(&db, wo, "2026-04-10T14:00:00Z", Some("2026-04-10T08:00:00Z")).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 1);
    let f = only_open_finding(&db, "WO_DOWNTIME_NEGATIVE_DURATION").await;
    assert_eq!(
        (
            f.severity.as_str(),
            f.domain.as_str(),
            f.record_class.as_str(),
            f.record_id
        ),
        ("error", "downtime", "work_order_downtime_segments", seg)
    );
    let d = details(&f);
    assert_eq!(
        (d["work_order_id"].as_i64(), d["segment_id"].as_i64()),
        (Some(wo), Some(seg))
    );
}

#[tokio::test]
async fn unclosed_downtime_segment_is_a_warning_finding() {
    let db = started_db().await;
    let wo = insert_wo(&db, "WO-OPEN-SEG", None).await;
    let seg = insert_segment(&db, wo, "2026-04-10T08:00:00Z", None).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 1);
    let f = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;
    assert_eq!((f.severity.as_str(), f.record_id), ("warning", seg));
}

#[tokio::test]
async fn downtime_sum_beyond_window_and_epsilon_is_found() {
    let db = started_db().await;
    // 2 h window, 1.5 h + 1 h of downtime = 2.5 h > 2 h + 0.25 h.
    let over = insert_wo(&db, "WO-OVER", Some(("2026-04-10T08:00:00Z", "2026-04-10T10:00:00Z"))).await;
    insert_segment(&db, over, "2026-04-10T08:00:00Z", Some("2026-04-10T09:30:00Z")).await;
    insert_segment(&db, over, "2026-04-10T12:00:00Z", Some("2026-04-10T13:00:00Z")).await;
    // 2 h window, 2.2 h of downtime: within the 0.25 h tolerance.
    let within = insert_wo(&db, "WO-WITHIN", Some(("2026-04-10T08:00:00Z", "2026-04-10T10:00:00Z"))).await;
    insert_segment(&db, within, "2026-04-10T08:00:00Z", Some("2026-04-10T10:12:00Z")).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 1);
    let f = only_open_finding(&db, "WO_DOWNTIME_SUM_EXCEEDS_WINDOW").await;
    assert_eq!(
        (f.severity.as_str(), f.record_class.as_str(), f.record_id),
        ("warning", "work_orders", over)
    );
    let d = details(&f);
    assert!(
        (d["segment_sum_hours"].as_f64().expect("sum") - 2.5).abs() < 1e-6,
        "{d}"
    );
    assert_eq!(d["window_hours"].as_f64(), Some(2.0));
    assert_eq!(d["epsilon_hours"].as_f64(), Some(0.25));
}

#[tokio::test]
async fn overlapping_segments_are_found_once_per_pair() {
    let db = started_db().await;
    let wo = insert_wo(&db, "WO-OVL", None).await;
    let a = insert_segment(&db, wo, "2026-04-10T08:00:00Z", Some("2026-04-10T12:00:00Z")).await;
    let b = insert_segment(&db, wo, "2026-04-10T10:00:00Z", Some("2026-04-10T14:00:00Z")).await;
    // Touching but not overlapping: starts exactly when b ends.
    insert_segment(&db, wo, "2026-04-10T14:00:00Z", Some("2026-04-10T15:00:00Z")).await;
    // Same times on another work order do not overlap with this one.
    let other = insert_wo(&db, "WO-OVL-OTHER", None).await;
    insert_segment(&db, other, "2026-04-10T09:00:00Z", Some("2026-04-10T11:00:00Z")).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 1);
    let f = only_open_finding(&db, "WO_DOWNTIME_OVERLAP").await;
    assert_eq!(f.record_id, a);
    let d = details(&f);
    assert_eq!(
        (
            d["segment_id_a"].as_i64(),
            d["segment_id_b"].as_i64(),
            d["work_order_id"].as_i64()
        ),
        (Some(a), Some(b), Some(wo))
    );
}

#[tokio::test]
async fn rerun_recomputes_open_findings_with_stable_identity() {
    let db = started_db().await;
    let wo = insert_wo(&db, "WO-RERUN", None).await;
    let seg = insert_segment(&db, wo, "2026-04-10T08:00:00Z", None).await;

    run_data_integrity_detectors(&db).await.expect("first run");
    let first = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;
    run_data_integrity_detectors(&db).await.expect("second run");
    let second = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;
    assert_eq!(
        second.entity_sync_id, first.entity_sync_id,
        "same check on the same record gives the same identity"
    );
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM data_integrity_findings", vec![]).await,
        1,
        "no duplicate rows across runs"
    );

    // Fix the data directly: the next run clears the open finding.
    exec(
        &db,
        "UPDATE work_order_downtime_segments SET ended_at = '2026-04-10T09:00:00Z' WHERE id = ?",
        vec![seg.into()],
    )
    .await;
    assert_eq!(run_data_integrity_detectors(&db).await.expect("third run"), 0);
    assert!(open_findings(&db).await.is_empty());
}

#[tokio::test]
async fn governed_reference_failure_mode_is_not_an_orphan() {
    let db = started_db().await;
    // Rows remapped by migration 114 (or written by close-out) carry reference_values ids, which the
    // declared foreign key to failure_codes does not describe.
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    let set_id = scalar_i64(
        &db,
        "SELECT rs.id FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.FAILURE_MODES' AND rs.status = 'published' ORDER BY rs.id LIMIT 1",
        vec![],
    )
    .await;
    exec(
        &db,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) \
         VALUES (?, 'DI_GOV_MODE', 'Governed mode', 1, 1)",
        vec![set_id.into()],
    )
    .await;
    let rv = scalar_i64(
        &db,
        "SELECT id FROM reference_values WHERE code = 'DI_GOV_MODE'",
        vec![],
    )
    .await;
    // Make sure no failure_codes row happens to carry the same id.
    exec(&db, "DELETE FROM failure_codes WHERE id = ?", vec![rv.into()]).await;
    let wo = insert_wo(&db, "WO-GOV", None).await;
    insert_failure_detail(&db, wo, Some(rv)).await;

    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 0);
}

// ── Waivers ─────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn waive_warning_checks_reason_and_version_then_audits() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-WAIVE", None).await;
    insert_segment(&db, wo, "2026-04-10T08:00:00Z", None).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;

    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 1, "   ", None), actor).await);
    assert!(msg.contains("reason required"), "{msg}");
    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 2, "ok", None), actor).await);
    assert!(msg.contains("stale row_version"), "{msg}");
    let missing = waive_data_integrity_finding(
        &db,
        WaiveDataIntegrityFindingInput {
            finding_id: f.id + 1_000,
            expected_row_version: 1,
            reason: "x".into(),
            approver_id: None,
        },
        actor,
    )
    .await;
    assert!(matches!(missing, Err(AppError::NotFound { .. })), "{missing:?}");
    assert_eq!(audit_rows(&db, f.id).await, 0, "refused waivers leave no audit row");
    assert_eq!(get_finding(&db, f.id).await.expect("get").expect("row").status, "open");

    let w = waive_data_integrity_finding(&db, waive(&f, 1, "Segment logged by mistake", None), actor)
        .await
        .expect("waive");
    assert_eq!((w.status.as_str(), w.row_version), ("waived", 2));
    assert_eq!(w.waiver_reason.as_deref(), Some("Segment logged by mistake"));
    assert_eq!(w.waiver_approver_id, Some(actor), "warning: the actor approves");
    assert!(w.cleared_at.is_some());

    let (action, audit_actor, before, after) = audit_row(&db, f.id).await;
    assert_eq!((action.as_str(), audit_actor), ("waive", actor));
    assert_eq!(before["finding"]["status"], "open");
    assert_eq!(before["finding"]["row_version"], 1);
    assert_eq!(after["finding"]["status"], "waived");
    assert_eq!(after["finding"]["waiver_reason"], "Segment logged by mistake");

    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 2, "again", None), actor).await);
    assert!(msg.contains("not open"), "{msg}");
    assert_eq!(audit_rows(&db, f.id).await, 1);

    // The waiver holds: the next run neither reopens nor duplicates the finding.
    run_data_integrity_detectors(&db).await.expect("rerun");
    assert!(open_findings(&db).await.is_empty());
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM data_integrity_findings", vec![]).await,
        1
    );
    assert_eq!(
        get_finding(&db, f.id).await.expect("get").expect("row").status,
        "waived"
    );
}

#[tokio::test]
async fn waive_error_finding_requires_a_second_person() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let approver = insert_user(&db, "di_supervisor").await;
    grant_integrity_repair(&db, approver).await;
    let wo = insert_wo(&db, "WO-WAIVE-ERR", None).await;
    insert_segment(&db, wo, "2026-04-10T14:00:00Z", Some("2026-04-10T08:00:00Z")).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_NEGATIVE_DURATION").await;
    assert_eq!(f.severity, "error");

    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 1, "known", None), actor).await);
    assert!(msg.contains("approver_id required"), "{msg}");
    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 1, "known", Some(actor)), actor).await);
    assert!(msg.contains("second-person approval"), "{msg}");
    assert_eq!(audit_rows(&db, f.id).await, 0);

    // AU-116: the second person must be able to waive it themselves.
    let bystander = insert_user(&db, "di_bystander").await;
    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 1, "known", Some(bystander)), actor).await);
    assert!(
        msg.starts_with("GATE_DI_INTEGRITY_APPROVER_NOT_ALLOWED"),
        "without the right: {msg}"
    );
    let departed = insert_user(&db, "di_departed").await;
    grant_integrity_repair(&db, departed).await;
    exec(
        &db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![departed.into()],
    )
    .await;
    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 1, "known", Some(departed)), actor).await);
    assert!(
        msg.starts_with("GATE_DI_INTEGRITY_APPROVER_NOT_ALLOWED"),
        "inactive: {msg}"
    );
    let msg = validation_text(waive_data_integrity_finding(&db, waive(&f, 1, "known", Some(999_999)), actor).await);
    assert!(
        msg.starts_with("GATE_DI_INTEGRITY_APPROVER_NOT_ALLOWED"),
        "unknown id: {msg}"
    );
    assert_eq!(audit_rows(&db, f.id).await, 0);
    assert_eq!(get_finding(&db, f.id).await.expect("get").expect("row").status, "open");

    let w = waive_data_integrity_finding(&db, waive(&f, 1, "Known clock skew", Some(approver)), actor)
        .await
        .expect("waive");
    assert_eq!((w.status.as_str(), w.waiver_approver_id), ("waived", Some(approver)));
    let (_, audit_actor, _, _) = audit_row(&db, f.id).await;
    assert_eq!(
        audit_actor, actor,
        "the audit names who acted, the finding who approved"
    );
}

// ── Repairs ─────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn repair_clear_failure_mode_nulls_the_mode_and_audits() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-FM", None).await;
    let (fd, inactive) = dangling_failure_detail(&db, wo).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "FK_ORPHAN_FAILURE_MODE").await;

    let r = apply_data_integrity_repair(&db, repair(&f, 1, "clear_failure_mode"), actor)
        .await
        .expect("repair");
    assert_eq!((r.status.as_str(), r.row_version), ("repaired", 2));
    assert!(r.cleared_at.is_some());
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM work_order_failure_details WHERE id = ? AND failure_mode_id IS NULL",
            vec![fd.into()],
        )
        .await,
        1
    );
    let (action, audit_actor, before, after) = audit_row(&db, f.id).await;
    assert_eq!((action.as_str(), audit_actor), ("clear_failure_mode", actor));
    assert_eq!(
        before["target"]["failure_detail"]["failure_mode_id"].as_i64(),
        Some(inactive)
    );
    assert_eq!(before["target"]["failure_detail"]["id"].as_i64(), Some(fd));
    assert!(after["target"]["failure_detail"]["failure_mode_id"].is_null());
    assert_eq!(before["finding"]["status"], "open");
    assert_eq!(after["finding"]["status"], "repaired");

    assert_eq!(run_data_integrity_detectors(&db).await.expect("rerun"), 0);
}

#[tokio::test]
async fn repair_refuses_to_clear_a_legacy_failure_code() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-LEGACY", None).await;
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    let legacy = insert_failure_code(&db, "DI_LEGACY_ONLY", "mode", true).await;
    exec(&db, "DELETE FROM reference_values WHERE id = ?", vec![legacy.into()]).await;
    exec(&db, "PRAGMA foreign_keys = ON", vec![]).await;
    let fd = insert_failure_detail_unchecked(&db, wo, legacy).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "FK_ORPHAN_FAILURE_MODE").await;

    let text = validation_text(apply_data_integrity_repair(&db, repair(&f, 1, "clear_failure_mode"), actor).await);
    assert!(text.contains("legacy failure code"), "{text}");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT failure_mode_id FROM work_order_failure_details WHERE id = ?",
            vec![fd.into()],
        )
        .await,
        legacy,
        "the value is kept"
    );
    assert_eq!(audit_rows(&db, f.id).await, 0);
    assert_eq!(only_open_finding(&db, "FK_ORPHAN_FAILURE_MODE").await.status, "open");
}

#[tokio::test]
async fn repair_rechecks_and_refuses_a_value_that_is_now_governed() {
    // The finding was raised on a dangling id; before the repair, the record was recoded with a
    // governed value. The repair must re-check the record, not trust the stale finding.
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-GOV", None).await;
    let (fd, _) = dangling_failure_detail(&db, wo).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "FK_ORPHAN_FAILURE_MODE").await;
    let governed = governed_mode(&db, "DI_RECODED").await;
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    exec(
        &db,
        "UPDATE work_order_failure_details SET failure_mode_id = ? WHERE id = ?",
        vec![governed.into(), fd.into()],
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON", vec![]).await;

    let text = validation_text(apply_data_integrity_repair(&db, repair(&f, 1, "clear_failure_mode"), actor).await);
    assert!(text.contains("governed list"), "{text}");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT failure_mode_id FROM work_order_failure_details WHERE id = ?",
            vec![fd.into()],
        )
        .await,
        governed
    );
    assert_eq!(audit_rows(&db, f.id).await, 0);
}

#[tokio::test]
async fn repair_swap_downtime_times_fixes_negative_segment() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-NEG", None).await;
    let seg = insert_segment(&db, wo, "2026-04-10T14:00:00Z", Some("2026-04-10T08:00:00Z")).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_NEGATIVE_DURATION").await;

    apply_data_integrity_repair(&db, repair(&f, 1, "swap_downtime_times"), actor)
        .await
        .expect("repair");
    assert_eq!(
        segment_times(&db, seg).await,
        (
            Some("2026-04-10T08:00:00Z".to_string()),
            Some("2026-04-10T14:00:00Z".to_string())
        )
    );
    let (action, _, before, after) = audit_row(&db, f.id).await;
    assert_eq!(action, "swap_downtime_times");
    assert_eq!(before["target"]["segment"]["started_at"], "2026-04-10T14:00:00Z");
    assert_eq!(after["target"]["segment"]["started_at"], "2026-04-10T08:00:00Z");
    assert_eq!(run_data_integrity_detectors(&db).await.expect("rerun"), 0);
}

#[tokio::test]
async fn repair_close_downtime_at_start_closes_open_segment() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-OPEN", None).await;
    let seg = insert_segment(&db, wo, "2026-04-10T08:00:00Z", None).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;

    apply_data_integrity_repair(&db, repair(&f, 1, "close_downtime_at_start"), actor)
        .await
        .expect("repair");
    assert_eq!(
        segment_times(&db, seg).await,
        (
            Some("2026-04-10T08:00:00Z".to_string()),
            Some("2026-04-10T08:00:00Z".to_string())
        )
    );
    let (action, _, before, after) = audit_row(&db, f.id).await;
    assert_eq!(action, "close_downtime_at_start");
    assert!(before["target"]["segment"]["ended_at"].is_null());
    assert_eq!(after["target"]["segment"]["ended_at"], "2026-04-10T08:00:00Z");
    assert_eq!(run_data_integrity_detectors(&db).await.expect("rerun"), 0);
}

#[tokio::test]
async fn repair_trim_overlap_end_ends_first_segment_at_second_start() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-OVL", None).await;
    let a = insert_segment(&db, wo, "2026-04-10T08:00:00Z", Some("2026-04-10T12:00:00Z")).await;
    let b = insert_segment(&db, wo, "2026-04-10T10:00:00Z", Some("2026-04-10T14:00:00Z")).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_OVERLAP").await;

    apply_data_integrity_repair(&db, repair(&f, 1, "trim_overlap_end"), actor)
        .await
        .expect("repair");
    assert_eq!(segment_times(&db, a).await.1.as_deref(), Some("2026-04-10T10:00:00Z"));
    assert_eq!(
        segment_times(&db, b).await,
        (
            Some("2026-04-10T10:00:00Z".to_string()),
            Some("2026-04-10T14:00:00Z".to_string())
        ),
        "second segment untouched"
    );
    let (action, _, before, after) = audit_row(&db, f.id).await;
    assert_eq!(action, "trim_overlap_end");
    assert_eq!(before["target"]["segment"]["ended_at"], "2026-04-10T12:00:00Z");
    assert_eq!(after["target"]["segment"]["ended_at"], "2026-04-10T10:00:00Z");
    assert_eq!(run_data_integrity_detectors(&db).await.expect("rerun"), 0);
}

#[tokio::test]
async fn repair_refuses_stale_version_without_touching_data() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-REP-STALE", None).await;
    let seg = insert_segment(&db, wo, "2026-04-10T14:00:00Z", Some("2026-04-10T08:00:00Z")).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_NEGATIVE_DURATION").await;

    let msg = validation_text(apply_data_integrity_repair(&db, repair(&f, 7, "swap_downtime_times"), actor).await);
    assert!(msg.contains("stale row_version"), "{msg}");
    assert_eq!(
        segment_times(&db, seg).await,
        (
            Some("2026-04-10T14:00:00Z".to_string()),
            Some("2026-04-10T08:00:00Z".to_string())
        )
    );
    let still = get_finding(&db, f.id).await.expect("get").expect("row");
    assert_eq!((still.status.as_str(), still.row_version), ("open", 1));
    assert_eq!(audit_rows(&db, f.id).await, 0);

    // The finding's version moves with each change: after a repair the old version is stale.
    apply_data_integrity_repair(&db, repair(&f, 1, "swap_downtime_times"), actor)
        .await
        .expect("repair");
    let msg = validation_text(apply_data_integrity_repair(&db, repair(&f, 1, "swap_downtime_times"), actor).await);
    assert!(msg.contains("not open"), "{msg}");
    assert_eq!(audit_rows(&db, f.id).await, 1);
}

#[tokio::test]
async fn repair_refuses_unsupported_kinds_and_missing_findings() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let over = insert_wo(
        &db,
        "WO-REP-SUM",
        Some(("2026-04-10T08:00:00Z", "2026-04-10T09:00:00Z")),
    )
    .await;
    insert_segment(&db, over, "2026-04-10T08:00:00Z", Some("2026-04-10T11:00:00Z")).await;
    let other = insert_wo(&db, "WO-REP-KIND", None).await;
    let open_seg = insert_segment(&db, other, "2026-04-10T08:00:00Z", None).await;
    run_data_integrity_detectors(&db).await.expect("run");

    let sum = only_open_finding(&db, "WO_DOWNTIME_SUM_EXCEEDS_WINDOW").await;
    let msg = validation_text(apply_data_integrity_repair(&db, repair(&sum, 1, "anything"), actor).await);
    assert!(msg.contains("no automated repair"), "{msg}");

    let unclosed = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;
    let msg =
        validation_text(apply_data_integrity_repair(&db, repair(&unclosed, 1, "swap_downtime_times"), actor).await);
    assert!(
        msg.contains("unsupported repair for WO_DOWNTIME_UNCLOSED / swap_downtime_times"),
        "{msg}"
    );
    assert_eq!(segment_times(&db, open_seg).await.1, None, "segment untouched");

    let missing = apply_data_integrity_repair(
        &db,
        ApplyDataIntegrityRepairInput {
            finding_id: 987_654,
            expected_row_version: 1,
            repair_kind: "clear_failure_mode".into(),
        },
        actor,
    )
    .await;
    assert!(matches!(missing, Err(AppError::NotFound { .. })), "{missing:?}");

    for f in [&sum, &unclosed] {
        let row = get_finding(&db, f.id).await.expect("get").expect("row");
        assert_eq!((row.status.as_str(), row.row_version), ("open", 1));
        assert_eq!(audit_rows(&db, f.id).await, 0);
    }
}

// ── Listing ─────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_open_findings_excludes_cleared_and_clamps_limit() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let wo = insert_wo(&db, "WO-LIST", None).await;
    insert_segment(&db, wo, "2026-04-10T08:00:00Z", None).await;
    insert_segment(&db, wo, "2026-04-11T08:00:00Z", None).await;
    insert_segment(&db, wo, "2026-04-12T14:00:00Z", Some("2026-04-12T08:00:00Z")).await;
    assert_eq!(run_data_integrity_detectors(&db).await.expect("run"), 3);
    assert_eq!(open_findings(&db).await.len(), 3);
    assert_eq!(
        list_open_findings(&db, 0).await.expect("list").len(),
        1,
        "limit raised to 1"
    );
    assert_eq!(list_open_findings(&db, 2).await.expect("list").len(), 2);

    let neg = only_open_finding(&db, "WO_DOWNTIME_NEGATIVE_DURATION").await;
    apply_data_integrity_repair(&db, repair(&neg, 1, "swap_downtime_times"), actor)
        .await
        .expect("repair");
    let open = open_findings(&db).await;
    assert_eq!(open.len(), 2);
    assert!(open
        .iter()
        .all(|f| f.finding_code == "WO_DOWNTIME_UNCLOSED" && f.status == "open"));
}

/// Below error severity a named approver other than the person waiving is checked as for an error
/// finding; naming nobody, or oneself, records the person waiving (review of f337d31a).
#[tokio::test]
async fn waive_warning_checks_a_named_approver() {
    let db = started_db().await;
    let actor = admin_id(&db).await;
    let bystander = insert_user(&db, "di_warn_bystander").await;
    let wo = insert_wo(&db, "WO-WAIVE-WARN-APPR", None).await;
    insert_segment(&db, wo, "2026-04-10T08:00:00Z", None).await;
    run_data_integrity_detectors(&db).await.expect("run");
    let f = only_open_finding(&db, "WO_DOWNTIME_UNCLOSED").await;
    assert_ne!(f.severity, "error");

    let msg = validation_text(
        waive_data_integrity_finding(&db, waive(&f, f.row_version, "known", Some(bystander)), actor).await,
    );
    assert!(msg.starts_with("GATE_DI_INTEGRITY_APPROVER_NOT_ALLOWED"), "{msg}");
    assert_eq!(audit_rows(&db, f.id).await, 0);

    let w = waive_data_integrity_finding(&db, waive(&f, f.row_version, "known", Some(actor)), actor)
        .await
        .expect("waive");
    assert_eq!(w.waiver_approver_id, Some(actor), "the person waiving approves it");
}
