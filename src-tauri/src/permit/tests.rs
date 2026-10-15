//! Permit / LOTO safety tests (PRD §6.23, WP-04: P0-7, P0-8).
//!
//! Covers: lifecycle transitions, the activation gate, the two-person rule on
//! isolation verification and removal verification, the close (removal) gate,
//! and the work-order start gate integration.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::permit::domain::{
    PermitHandoverLogInput, PermitIsolation, PermitIsolationActionInput, PermitIsolationUpsertInput,
    PermitSuspendInput, WorkPermit, WorkPermitCreateInput, WorkPermitStatusInput, WorkPermitUpdateInput,
};
use crate::permit::queries;
use crate::wo::domain::WoCreateInput;
use crate::wo::execution::{self, WoAssignInput, WoPlanInput, WoStartInput};
use crate::wo::workflow::actions::mark_ready::{mark_wo_ready, WoMarkReadyInput};
use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

/// Applier and verifier are different people; tests never reuse one for the other
/// unless they assert the two-person rejection.
const APPLIER: i64 = 1;
const VERIFIER: i64 = 2;

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    // APPLIER is the seeded admin; VERIFIER is a second real account (the isolation
    // actor columns reference `user_accounts`).
    assert_eq!(admin_id(&db).await, APPLIER, "seeded admin is the applier");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active,          is_admin, force_password_change, created_at, updated_at)          VALUES (?, 'user-sync-ptw-verifier', 'ptw_verifier', 'PTW Verifier', 'local', 1, 0, 0,          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [VERIFIER.into()],
    ))
    .await
    .expect("insert verifier account");
    db
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row");
    row.try_get_by_index::<i64>(0).expect("i64")
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1").await
}

async fn type_id(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(db, &format!("SELECT id FROM permit_types WHERE code = '{code}'")).await
}

async fn insert_equipment(db: &DatabaseConnection, code: &str) -> i64 {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at)          VALUES (1, 'permit-test-model-001', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active,          created_at, updated_at) VALUES (1, 'permit-test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("org scaffolding");
    }
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_nodes          (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id)          VALUES (1, 'permit-test-org-001', 'SITE-PTW', 'Permit Test Site', 1, 'active',                  datetime('now'), datetime('now'), 1)"
            .to_string(),
    ))
    .await
    .expect("insert org node");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,          created_at, updated_at)          VALUES (?, ?, 'Permit test asset', 'active_in_service', 1,          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [format!("eq-sync-{code}").into(), code.into()],
    ))
    .await
    .expect("insert equipment");
    scalar_i64(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
}

fn future_expiry() -> String {
    (chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339()
}

async fn create_permit(db: &DatabaseConnection, type_code: &str, asset_id: i64) -> WorkPermit {
    queries::create_work_permit(
        db,
        WorkPermitCreateInput {
            permit_type_id: type_id(db, type_code).await,
            asset_id: Some(asset_id),
            entity_id: Some(1),
            linked_work_order_id: None,
            expires_at: Some(future_expiry()),
        },
    )
    .await
    .expect("create permit")
}

async fn set_status(db: &DatabaseConnection, wp: &WorkPermit, status: &str) -> Result<WorkPermit, AppError> {
    queries::set_work_permit_status(
        db,
        WorkPermitStatusInput {
            id: wp.id,
            status: status.into(),
            expected_row_version: wp.row_version,
        },
        APPLIER,
    )
    .await
}

/// `draft` → `pending_review` → `approved` → `issued`
async fn advance_to_issued(db: &DatabaseConnection, mut wp: WorkPermit) -> WorkPermit {
    for s in ["pending_review", "approved", "issued"] {
        wp = set_status(db, &wp, s).await.unwrap_or_else(|e| panic!("-> {s}: {e:?}"));
    }
    wp
}

async fn add_isolation(db: &DatabaseConnection, permit_id: i64, point: &str) -> PermitIsolation {
    queries::upsert_permit_isolation(
        db,
        PermitIsolationUpsertInput {
            id: None,
            permit_id,
            isolation_point: point.into(),
            energy_type: "electrical".into(),
            isolation_method: "breaker lock-out".into(),
            lock_number: None,
        },
    )
    .await
    .expect("add isolation")
}

fn action(iso: &PermitIsolation) -> PermitIsolationActionInput {
    PermitIsolationActionInput {
        isolation_id: iso.id,
        expected_row_version: iso.row_version,
        lock_number: None,
    }
}

async fn apply_and_verify(db: &DatabaseConnection, iso: &PermitIsolation) -> PermitIsolation {
    let applied = queries::apply_permit_isolation(db, action(iso), APPLIER)
        .await
        .expect("apply");
    queries::verify_permit_isolation(db, action(&applied), VERIFIER)
        .await
        .expect("verify")
}

fn assert_validation(res: Result<impl std::fmt::Debug, AppError>, needle: &str) {
    match res {
        Err(AppError::ValidationFailed(msgs)) => assert!(
            msgs.iter().any(|m| m.contains(needle)),
            "expected message containing {needle:?}, got {msgs:?}"
        ),
        other => panic!("expected ValidationFailed({needle:?}), got {other:?}"),
    }
}

/// An issued LOTO permit with one applied + verified isolation, activated.
async fn active_loto_permit(db: &DatabaseConnection, asset_code: &str) -> (WorkPermit, PermitIsolation) {
    let asset = insert_equipment(db, asset_code).await;
    let wp = advance_to_issued(db, create_permit(db, "loto", asset).await).await;
    let iso = apply_and_verify(db, &add_isolation(db, wp.id, "MCC-1 breaker").await).await;
    let wp = set_status(db, &wp, "active").await.expect("activate");
    (wp, iso)
}

// ── Lifecycle ────────────────────────────────────────────────────────

#[tokio::test]
async fn create_assigns_code_labels_and_draft_status() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-01").await;
    let wp = create_permit(&db, "hot_work", asset).await;
    assert_eq!(wp.status, "draft");
    assert!(wp.code.starts_with("PTW-"), "code {}", wp.code);
    assert_eq!(wp.permit_type_code.as_deref(), Some("hot_work"));
    assert_eq!(wp.asset_code.as_deref(), Some("EQ-PTW-01"));
    assert!(wp.requested_at.is_some());
}

#[tokio::test]
async fn create_rejects_past_expiry() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-02").await;
    let res = queries::create_work_permit(
        &db,
        WorkPermitCreateInput {
            permit_type_id: type_id(&db, "hot_work").await,
            asset_id: Some(asset),
            entity_id: Some(1),
            linked_work_order_id: None,
            expires_at: Some("2020-01-01T00:00:00Z".into()),
        },
    )
    .await;
    assert_validation(res, "future");
}

#[tokio::test]
async fn full_lifecycle_to_handed_back_stamps_timestamps() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-03").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    assert!(wp.issued_at.is_some());
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    assert!(wp.activated_at.is_some());
    let wp = set_status(&db, &wp, "closed").await.expect("close (no isolations)");
    assert!(wp.closed_at.is_some());

    assert_validation(set_status(&db, &wp, "handed_back").await, "handover");
    queries::append_permit_handover_log(
        &db,
        PermitHandoverLogInput {
            permit_id: wp.id,
            handed_from_role: "Maintenance".into(),
            handed_to_role: "Operations".into(),
            confirmation_note: "Area clean".into(),
        },
    )
    .await
    .expect("handover log");
    let wp = set_status(&db, &wp, "handed_back").await.expect("hand back");
    assert!(wp.handed_back_at.is_some());
}

#[tokio::test]
async fn invalid_transitions_are_rejected() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-04").await;
    let wp = create_permit(&db, "hot_work", asset).await;
    assert_validation(set_status(&db, &wp, "active").await, "not allowed");
    assert_validation(set_status(&db, &wp, "closed").await, "not allowed");
    let wp = advance_to_issued(&db, wp).await;
    // active -> suspended must use suspend_work_permit
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    assert_validation(set_status(&db, &wp, "suspended").await, "not allowed");
}

#[tokio::test]
async fn stale_row_version_is_rejected() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-05").await;
    let wp = create_permit(&db, "hot_work", asset).await;
    let _ = set_status(&db, &wp, "pending_review").await.expect("review");
    assert_validation(set_status(&db, &wp, "approved").await, "row_version");
}

#[tokio::test]
async fn details_are_frozen_once_active() {
    let db = setup().await;
    let (wp, _) = active_loto_permit(&db, "EQ-PTW-06").await;
    let res = queries::update_work_permit(
        &db,
        WorkPermitUpdateInput {
            id: wp.id,
            linked_work_order_id: None,
            asset_id: None,
            entity_id: None,
            expires_at: Some(future_expiry()),
            expected_row_version: wp.row_version,
        },
    )
    .await;
    assert_validation(res, "cannot be edited");
}

#[tokio::test]
async fn suspend_and_reinstate_records_session_actor() {
    let db = setup().await;
    let (wp, _) = active_loto_permit(&db, "EQ-PTW-07").await;
    let (wp, susp) = queries::suspend_work_permit(
        &db,
        PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Gas alarm".into(),
            reactivation_conditions: Some("Re-test atmosphere".into()),
        },
        VERIFIER,
    )
    .await
    .expect("suspend");
    assert_eq!(wp.status, "suspended");
    assert_eq!(susp.suspended_by_id, VERIFIER);

    let wp = set_status(&db, &wp, "revalidation_required").await.expect("revalidate");
    let wp = set_status(&db, &wp, "active").await.expect("reinstate");
    assert_eq!(wp.status, "active");
    let susps = queries::list_permit_suspensions(&db, wp.id).await.expect("list");
    assert_eq!(susps[0].reinstated_by_id, Some(APPLIER));
    assert!(susps[0].reinstated_at.is_some());
}

// ── Activation gate ──────────────────────────────────────────────────

#[tokio::test]
async fn loto_activation_requires_an_isolation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-10").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    assert_validation(set_status(&db, &wp, "active").await, "at least one isolation");
}

#[tokio::test]
async fn activation_requires_every_isolation_verified() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-11").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "Valve V-12").await;
    assert_validation(set_status(&db, &wp, "active").await, "verified_at");
    let applied = queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("apply");
    assert_validation(set_status(&db, &wp, "active").await, "verified_at");
    queries::verify_permit_isolation(&db, action(&applied), VERIFIER)
        .await
        .expect("verify");
    set_status(&db, &wp, "active").await.expect("activate once verified");
}

#[tokio::test]
async fn activation_rejects_expired_permit() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-12").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET expires_at = '2020-01-01T00:00:00Z' WHERE id = ?",
        [wp.id.into()],
    ))
    .await
    .expect("expire");
    assert_validation(set_status(&db, &wp, "active").await, "expired");
}

#[tokio::test]
async fn isolations_cannot_be_added_to_an_active_permit() {
    let db = setup().await;
    let (wp, _) = active_loto_permit(&db, "EQ-PTW-13").await;
    let res = queries::upsert_permit_isolation(
        &db,
        PermitIsolationUpsertInput {
            id: None,
            permit_id: wp.id,
            isolation_point: "Late addition".into(),
            energy_type: "hydraulic".into(),
            isolation_method: "valve lock".into(),
            lock_number: None,
        },
    )
    .await;
    assert_validation(res, "define isolations");
}

// ── Two-person rule ──────────────────────────────────────────────────

#[tokio::test]
async fn applier_cannot_verify_own_isolation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-20").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "Pump P-3 isolator").await;
    let applied = queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("apply");
    assert_eq!(applied.applied_by_id, Some(APPLIER));
    assert_validation(
        queries::verify_permit_isolation(&db, action(&applied), APPLIER).await,
        "Two-person rule",
    );
    let verified = queries::verify_permit_isolation(&db, action(&applied), VERIFIER)
        .await
        .expect("second person verifies");
    assert_eq!(verified.verified_by_id, Some(VERIFIER));
    assert!(verified.verified_at.is_some());
}

#[tokio::test]
async fn verify_requires_application_first() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-21").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "Conveyor C-1").await;
    assert_validation(
        queries::verify_permit_isolation(&db, action(&iso), VERIFIER).await,
        "must be applied",
    );
}

#[tokio::test]
async fn applied_isolation_cannot_be_edited() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-22").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "Fan F-2").await;
    queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("apply");
    let res = queries::upsert_permit_isolation(
        &db,
        PermitIsolationUpsertInput {
            id: Some(iso.id),
            permit_id: wp.id,
            isolation_point: "Renamed".into(),
            energy_type: "electrical".into(),
            isolation_method: "breaker lock-out".into(),
            lock_number: None,
        },
    )
    .await;
    assert_validation(res, "cannot be edited");
}

#[tokio::test]
async fn applier_cannot_verify_own_lock_removal() {
    let db = setup().await;
    let (_, iso) = active_loto_permit(&db, "EQ-PTW-23").await;
    assert_validation(
        queries::verify_permit_isolation_removal(&db, action(&iso), APPLIER).await,
        "Two-person rule",
    );
    let removed = queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("removal verified by second person");
    assert_eq!(removed.removal_verified_by_id, Some(VERIFIER));
    assert!(removed.removal_verified_at.is_some());
}

#[tokio::test]
async fn removal_cannot_be_verified_before_activation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-24").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "Line L-4").await).await;
    assert_validation(
        queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER).await,
        "verify isolation removal",
    );
}

// ── Removal (close) gate ─────────────────────────────────────────────

#[tokio::test]
async fn close_requires_every_lock_removal_verified() {
    let db = setup().await;
    let (wp, iso) = active_loto_permit(&db, "EQ-PTW-30").await;
    assert_validation(set_status(&db, &wp, "closed").await, "lock removal");
    queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("removal");
    let wp = set_status(&db, &wp, "closed").await.expect("close after removal");
    assert_eq!(wp.status, "closed");
}

// ── WO gate integration ──────────────────────────────────────────────

/// Creates a WO that requires a permit, on `asset_id`, advanced to `ready`.
async fn assigned_wo_requiring_permit(db: &DatabaseConnection, asset_id: i64) -> (i64, i64) {
    let actor = admin_id(db).await;
    let wo = crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(asset_id),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: Some(1),
            planner_id: None,
            urgency_id: Some(3),
            title: "Replace breaker".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor,
            requires_permit: Some(true),
        },
    )
    .await
    .expect("create WO");
    let wo = submit_wo(
        db,
        WoSubmitInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit");
    let wo = execution::plan_wo(
        db,
        WoPlanInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            planner_id: actor,
            planned_start: Some("2026-04-10T08:00:00Z".into()),
            planned_end: Some("2026-04-10T16:00:00Z".into()),
            shift: None,
            expected_duration_hours: Some(8.0),
            urgency_id: None,
            planned_downtime_hours: None,
        },
    )
    .await
    .expect("plan");
    let wo = execution::assign_wo(
        db,
        WoAssignInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            assigned_team_id: None,
            primary_responsible_id: Some(actor),
            scheduled_at: None,
            override_reason: "permit test".into(),
        },
    )
    .await
    .expect("assign");
    let wo = mark_wo_ready(
        db,
        WoMarkReadyInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("ready");
    (wo.id, wo.row_version)
}

async fn start(db: &DatabaseConnection, wo_id: i64, rv: i64) -> Result<(), AppError> {
    let actor = admin_id(db).await;
    execution::start_wo(
        db,
        WoStartInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
        },
    )
    .await
    .map(|_| ())
}

async fn create_from_wo(db: &DatabaseConnection, type_code: &str, wo_id: i64) -> Result<WorkPermit, AppError> {
    queries::create_work_permit(
        db,
        WorkPermitCreateInput {
            permit_type_id: type_id(db, type_code).await,
            asset_id: None,
            entity_id: None,
            linked_work_order_id: Some(wo_id),
            expires_at: Some(future_expiry()),
        },
    )
    .await
}

#[tokio::test]
async fn wo_gate_unblocks_only_with_active_verified_permit() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-40").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;

    // No permit at all → blocked.
    assert!(matches!(
        start(&db, wo_id, rv).await,
        Err(AppError::ValidationFailed(_))
    ));

    // Permit created from the WO: asset and entity derived from the WO.
    let wp = create_from_wo(&db, "loto", wo_id).await.expect("create from WO");
    assert_eq!(wp.asset_id, asset);
    assert!(wp.linked_work_order_code.is_some());
    // The WO analytics feed lists the permit (Phase 6.4).
    let links = crate::wo::analytics::load_permit_links(&db, wo_id)
        .await
        .expect("permit links");
    assert_eq!(links.len(), 1);
    assert_eq!((links[0].id, links[0].code.as_str()), (wp.id, wp.code.as_str()));
    assert_eq!(links[0].permit_type_code.as_deref(), Some("loto"));

    // A second open permit on the same WO is refused.
    assert_validation(create_from_wo(&db, "loto", wo_id).await, "already has an open permit");

    // Issued but not active → still blocked.
    let wp = advance_to_issued(&db, wp).await;
    assert!(matches!(
        start(&db, wo_id, rv).await,
        Err(AppError::ValidationFailed(_))
    ));

    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-7").await).await;
    set_status(&db, &wp, "active").await.expect("activate");

    start(&db, wo_id, rv).await.expect("WO starts with active permit");
    let in_progress = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
             WHERE wo.id = {wo_id} AND s.code = 'in_progress'"
        ),
    )
    .await;
    assert_eq!(in_progress, 1);
}

#[tokio::test]
async fn wo_gate_blocks_suspended_permit() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-41").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = create_from_wo(&db, "hot_work", wo_id).await.expect("create");
    let wp = set_status(&db, &advance_to_issued(&db, wp).await, "active")
        .await
        .expect("activate");
    queries::suspend_work_permit(
        &db,
        PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Weather".into(),
            reactivation_conditions: None,
        },
        APPLIER,
    )
    .await
    .expect("suspend");
    assert_validation(start(&db, wo_id, rv).await, "suspendu");
}

#[tokio::test]
async fn wo_gate_blocks_after_locks_removed() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-42").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("create")).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-9").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("remove");
    assert_validation(start(&db, wo_id, rv).await, "already been removed");
}

#[tokio::test]
async fn gate_prefers_live_permit_over_newer_cancelled_one() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-43").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let first = create_from_wo(&db, "hot_work", wo_id).await.expect("first");
    set_status(&db, &first, "cancelled").await.expect("cancel");
    let second = create_from_wo(&db, "hot_work", wo_id)
        .await
        .expect("second after cancel");
    let linked = queries::get_work_permit_linked_to_work_order(&db, wo_id)
        .await
        .expect("lookup")
        .expect("linked");
    assert_eq!(linked.id, second.id);
}

#[tokio::test]
async fn locks_cannot_be_removed_while_linked_work_is_running() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-44").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("create")).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-11").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    start(&db, wo_id, rv).await.expect("WO starts");

    assert_validation(
        queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER).await,
        "still in progress",
    );

    // Once the work is no longer being executed, the removal can be verified.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'completed') \
         WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("complete WO");
    queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("removal after work completed");
}
