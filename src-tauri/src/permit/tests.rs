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
pub(super) const APPLIER: i64 = 1;
pub(super) const VERIFIER: i64 = 2;

/// A permit-type editor without the administrator right / step-up.
const EDITOR: queries::PermitTypeWriter = queries::PermitTypeWriter {
    actor_id: APPLIER,
    may_relax_isolation: false,
};
/// An administrator with a valid step-up (checked by the command).
const ADMIN_STEP_UP: queries::PermitTypeWriter = queries::PermitTypeWriter {
    actor_id: APPLIER,
    may_relax_isolation: true,
};

pub(super) async fn setup() -> DatabaseConnection {
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

pub(super) async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row");
    row.try_get_by_index::<i64>(0).expect("i64")
}

pub(super) async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1").await
}

/// Id of a value of a LOTO list (`permit::energy_reference`).
async fn list_value_id(db: &DatabaseConnection, domain: &str, code: &str) -> i64 {
    scalar_i64(
        db,
        &format!(
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id              JOIN reference_domains d ON d.id = rs.domain_id              WHERE d.code = '{domain}' AND rv.code = '{code}'"
        ),
    )
    .await
}

async fn energy(db: &DatabaseConnection, code: &str) -> Option<i64> {
    Some(list_value_id(db, crate::permit::energy_reference::ENERGY_TYPES_DOMAIN, code).await)
}

async fn method(db: &DatabaseConnection, code: &str) -> Option<i64> {
    Some(list_value_id(db, crate::permit::energy_reference::ISOLATION_METHODS_DOMAIN, code).await)
}

pub(super) async fn type_id(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(db, &format!("SELECT id FROM permit_types WHERE code = '{code}'")).await
}

pub(super) async fn insert_equipment(db: &DatabaseConnection, code: &str) -> i64 {
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

pub(super) fn future_expiry() -> String {
    (chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339()
}

pub(super) async fn create_permit(db: &DatabaseConnection, type_code: &str, asset_id: i64) -> WorkPermit {
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

pub(super) async fn set_status(db: &DatabaseConnection, wp: &WorkPermit, status: &str) -> Result<WorkPermit, AppError> {
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
pub(super) async fn advance_to_issued(db: &DatabaseConnection, mut wp: WorkPermit) -> WorkPermit {
    for s in ["pending_review", "approved", "issued"] {
        wp = set_status(db, &wp, s).await.unwrap_or_else(|e| panic!("-> {s}: {e:?}"));
    }
    wp
}

pub(super) async fn add_isolation(db: &DatabaseConnection, permit_id: i64, point: &str) -> PermitIsolation {
    queries::upsert_permit_isolation(
        db,
        PermitIsolationUpsertInput {
            id: None,
            permit_id,
            isolation_point: point.into(),
            energy_type_id: energy(db, "ELECTRICAL").await,
            isolation_method_id: method(db, "BREAKER_LOCKOUT").await,
            lock_number: None,
        },
    )
    .await
    .expect("add isolation")
}

pub(super) fn action(iso: &PermitIsolation) -> PermitIsolationActionInput {
    PermitIsolationActionInput {
        isolation_id: iso.id,
        expected_row_version: iso.row_version,
        lock_number: None,
    }
}

pub(super) async fn apply_and_verify(db: &DatabaseConnection, iso: &PermitIsolation) -> PermitIsolation {
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
        // Coded refusals (start gate, activation): the code or the message names the rule.
        Err(AppError::OrgValidationFailed(issues)) => assert!(
            issues.iter().any(|i| i.code == needle || i.message.contains(needle)),
            "expected an issue {needle:?}, got {issues:?}"
        ),
        other => panic!("expected a refusal {needle:?}, got {other:?}"),
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
    assert_validation(set_status(&db, &wp, "active").await, "LOTO_NO_ISOLATION");
}

#[tokio::test]
async fn activation_requires_every_isolation_verified() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-11").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "Valve V-12").await;
    assert_validation(set_status(&db, &wp, "active").await, "LOTO_NOT_VERIFIED");
    let applied = queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("apply");
    assert_validation(set_status(&db, &wp, "active").await, "LOTO_NOT_VERIFIED");
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
    assert_validation(set_status(&db, &wp, "active").await, "PERMIT_EXPIRED");
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
            energy_type_id: energy(&db, "HYDRAULIC").await,
            isolation_method_id: method(&db, "VALVE_LOCKOUT").await,
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
            energy_type_id: energy(&db, "ELECTRICAL").await,
            isolation_method_id: method(&db, "BREAKER_LOCKOUT").await,
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
pub(super) async fn assigned_wo_requiring_permit(db: &DatabaseConnection, asset_id: i64) -> (i64, i64) {
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
            urgency_code: Some("normal".into()),
            subtype_code: None,
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
            shift_class_version_id: None,
            clear_shift: false,
            expected_duration_hours: Some(8.0),
            planned_crew_size: None,
            clear_crew_size: false,
            urgency_code: None,
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

pub(super) async fn create_from_wo(
    db: &DatabaseConnection,
    type_code: &str,
    wo_id: i64,
) -> Result<WorkPermit, AppError> {
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
    assert_validation(start(&db, wo_id, rv).await, "PERMIT_REQUIRED_TO_START");

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
    assert_validation(start(&db, wo_id, rv).await, "PERMIT_NOT_ACTIVE");

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
    assert_validation(start(&db, wo_id, rv).await, "PERMIT_SUSPENDED");
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
    assert_validation(start(&db, wo_id, rv).await, "LOTO_ISOLATION_REMOVED");
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

// ── Governed LOTO lists and the energy-isolation rule (migration 225) ──

#[tokio::test]
async fn the_isolation_rule_is_the_type_flag_and_the_loto_type_keeps_it() {
    let db = setup().await;
    let types = queries::list_permit_types(&db).await.expect("types");
    let loto = types.iter().find(|t| t.code == "loto").expect("loto type").clone();
    let hot = types
        .iter()
        .find(|t| t.code == "hot_work")
        .expect("hot work type")
        .clone();
    assert!(loto.requires_energy_isolation, "migration 225 sets the flag on loto");
    assert!(!hot.requires_energy_isolation);

    // A hot-work permit starts without an isolation...
    let asset = insert_equipment(&db, "EQ-PTW-60").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    assert!(!wp.permit_type_requires_energy_isolation);
    set_status(&db, &wp, "active")
        .await
        .expect("hot work without isolation");

    // ...until the plant says the type needs one: then the same gate as LOTO refuses it.
    let updated = queries::upsert_permit_type(
        &db,
        crate::permit::domain::PermitTypeUpsertInput {
            id: Some(hot.id),
            code: hot.code.clone(),
            name: hot.name.clone(),
            description: Some(hot.description.clone()),
            requires_hse_approval: hot.requires_hse_approval,
            requires_operations_approval: hot.requires_operations_approval,
            requires_atmospheric_test: hot.requires_atmospheric_test,
            max_duration_hours: hot.max_duration_hours,
            mandatory_ppe_ids_json: Some(hot.mandatory_ppe_ids_json.clone()),
            mandatory_control_rules_json: Some(hot.mandatory_control_rules_json.clone()),
            requires_energy_isolation: Some(true),
            disable_isolation_reason: None,
        },
        EDITOR,
    )
    .await
    .expect("flag hot work");
    assert!(updated.requires_energy_isolation);
    let asset = insert_equipment(&db, "EQ-PTW-61").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    assert_validation(set_status(&db, &wp, "active").await, "LOTO_NO_ISOLATION");

    // An update that does not send the flag keeps it (no silent reset of the LOTO rule).
    let kept = queries::upsert_permit_type(
        &db,
        crate::permit::domain::PermitTypeUpsertInput {
            id: Some(loto.id),
            code: loto.code.clone(),
            name: loto.name.clone(),
            description: Some(loto.description.clone()),
            requires_hse_approval: loto.requires_hse_approval,
            requires_operations_approval: loto.requires_operations_approval,
            requires_atmospheric_test: loto.requires_atmospheric_test,
            max_duration_hours: loto.max_duration_hours,
            mandatory_ppe_ids_json: None,
            mandatory_control_rules_json: None,
            requires_energy_isolation: None,
            disable_isolation_reason: None,
        },
        EDITOR,
    )
    .await
    .expect("rename loto");
    assert!(kept.requires_energy_isolation);
}

#[tokio::test]
async fn isolations_record_list_links_only() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-62").await;
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "Q4 feeder").await;
    assert_eq!(iso.energy_type, "", "no original entry on a new isolation");
    assert_eq!(iso.isolation_method, "");
    assert_eq!(iso.energy_type_code.as_deref(), Some("ELECTRICAL"));
    assert_eq!(iso.energy_type_label.as_deref(), Some("Électrique"));
    assert!(iso.isolation_method_label.is_some());
    assert!(!iso.mapping_review_required);

    let input = |energy_id: Option<i64>, method_id: Option<i64>| PermitIsolationUpsertInput {
        id: None,
        permit_id: wp.id,
        isolation_point: "Valve V-2".into(),
        energy_type_id: energy_id,
        isolation_method_id: method_id,
        lock_number: None,
    };
    let valve = method(&db, "VALVE_LOCKOUT").await;
    assert_validation(
        queries::upsert_permit_isolation(&db, input(None, valve)).await,
        "energy_type_id is required",
    );
    // A method id is not an energy type.
    assert_validation(
        queries::upsert_permit_isolation(&db, input(valve, valve)).await,
        "not a value of PERMIT.ENERGY_TYPES",
    );
    let blind = method(&db, "BLIND").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_values SET is_active = 0 WHERE id = ?",
        [blind.into()],
    ))
    .await
    .expect("retire method");
    assert_validation(
        queries::upsert_permit_isolation(&db, input(energy(&db, "HYDRAULIC").await, blind)).await,
        "no longer active",
    );

    let options = crate::permit::energy_reference::list_isolation_options(&db)
        .await
        .expect("options");
    assert_eq!(
        options.energy_types.len(),
        crate::permit::energy_reference::ENERGY_TYPES.len()
    );
    assert!(
        options.isolation_methods.iter().all(|m| m.code != "BLIND"),
        "inactive method not offered"
    );
    assert_eq!(
        options
            .energy_types
            .iter()
            .find(|e| e.code == "PNEUMATIC")
            .and_then(|e| e.label_en.clone()),
        Some("Pneumatic".to_string())
    );
}

#[tokio::test]
async fn typed_texts_are_linked_kept_and_unmatched_ones_flagged() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-63").await;
    let wp = create_permit(&db, "loto", asset).await;
    let rows: [(&str, &str, &str); 5] = [
        ("P1", "Électrique 400V", "Cadenas disjoncteur"),
        ("P2", "VAPEUR", "vanne fermée"),
        ("P3", "plasma", "clé captive"),
        ("P4", "", ""),
        ("P5", "pneumatic", "Clé captive"),
    ];
    for (i, (point, e, m)) in rows.iter().enumerate() {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO permit_isolations (entity_sync_id, permit_id, isolation_point, energy_type, \
             isolation_method, row_version) VALUES (?, ?, ?, ?, ?, 1)",
            [
                format!("iso-legacy-{i}").into(),
                wp.id.into(),
                (*point).into(),
                (*e).into(),
                (*m).into(),
            ],
        ))
        .await
        .expect("legacy isolation");
    }
    let report = crate::permit::energy_reference::map_isolation_texts(&db)
        .await
        .expect("map");
    assert_eq!(report.rows, 5);
    assert_eq!(
        report.methods_created, 1,
        "one plant method for both spellings of 'clé captive'"
    );
    assert_eq!(
        report.to_review, 3,
        "P3 (unknown energy + created method), P4 (empty texts), P5 (created method)"
    );

    let isos = queries::list_permit_isolations(&db, wp.id).await.expect("list");
    let by_point = |p: &str| isos.iter().find(|i| i.isolation_point == p).expect("row").clone();
    let p1 = by_point("P1");
    assert_eq!(p1.energy_type_code.as_deref(), Some("ELECTRICAL"));
    assert_eq!(p1.energy_type, "Électrique 400V", "original entry kept as history");
    assert!(!p1.mapping_review_required);
    assert_eq!(by_point("P2").energy_type_code.as_deref(), Some("THERMAL"));
    let p3 = by_point("P3");
    assert_eq!(p3.energy_type_code.as_deref(), Some("OTHER"));
    assert!(p3.mapping_review_required);
    assert_eq!(p3.energy_type, "plasma");
    let p4 = by_point("P4");
    assert!(p4.energy_type_id.is_none() && p4.mapping_review_required);
    assert_eq!(by_point("P5").isolation_method_id, p3.isolation_method_id);

    // The created method is a plant value flagged for review; the original spelling of a matched
    // text is a legacy alias.
    let flagged = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM reference_values WHERE label = 'clé captive' \
           AND json_extract(metadata_json, '$.review_required') = 1",
    )
    .await;
    assert_eq!(flagged, 1);
    let aliases = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM reference_aliases WHERE alias_type = 'legacy' AND alias_label = 'Électrique 400V'",
    )
    .await;
    assert_eq!(aliases, 1);

    // A second run (the startup pass) changes nothing.
    let again = crate::permit::energy_reference::map_isolation_texts(&db)
        .await
        .expect("rerun");
    assert_eq!(again.rows, 0);

    // A person choosing the values clears the review flag; the original text stays.
    let edited = queries::upsert_permit_isolation(
        &db,
        PermitIsolationUpsertInput {
            id: Some(p3.id),
            permit_id: wp.id,
            isolation_point: "P3".into(),
            energy_type_id: energy(&db, "RADIATION").await,
            isolation_method_id: method(&db, "OTHER").await,
            lock_number: None,
        },
    )
    .await
    .expect("review P3");
    assert!(!edited.mapping_review_required);
    assert_eq!(edited.energy_type, "plasma");
    assert_eq!(edited.energy_type_code.as_deref(), Some("RADIATION"));
}

#[test]
fn energy_and_method_texts_match_in_french_and_english() {
    use crate::permit::energy_reference::{match_entry, ENERGY_TYPES, ISOLATION_METHODS};
    assert_eq!(match_entry(ENERGY_TYPES, "électricité"), Some("ELECTRICAL"));
    assert_eq!(match_entry(ENERGY_TYPES, "Air comprimé 6 bar"), Some("PNEUMATIC"));
    assert_eq!(match_entry(ENERGY_TYPES, "Gravity"), Some("GRAVITATIONAL"));
    assert_eq!(match_entry(ENERGY_TYPES, "ressort comprimé"), Some("STORED"));
    // Two energies named in one text: not guessed.
    assert_eq!(match_entry(ENERGY_TYPES, "électrique et hydraulique"), None);
    assert_eq!(match_entry(ENERGY_TYPES, "  "), None);
    assert_eq!(
        match_entry(ISOLATION_METHODS, "Double block & bleed"),
        Some("DOUBLE_BLOCK_BLEED")
    );
    assert_eq!(match_entry(ISOLATION_METHODS, "joint plein DN50"), Some("BLIND"));
}

#[tokio::test]
async fn permit_search_runs_on_the_server_over_every_permit() {
    let db = setup().await;
    let a = insert_equipment(&db, "EQ-PTW-SEARCH-A").await;
    let b = insert_equipment(&db, "EQ-PTW-OTHER-B").await;
    let pa = create_permit(&db, "loto", a).await;
    create_permit(&db, "hot_work", b).await;
    let filter = |search: &str| crate::permit::domain::WorkPermitListFilter {
        search: Some(search.into()),
        limit: Some(5),
        ..Default::default()
    };
    let hits = queries::list_work_permits(&db, filter("search-a"))
        .await
        .expect("search");
    assert_eq!(hits.iter().map(|p| p.id).collect::<Vec<_>>(), vec![pa.id]);
    // LIKE wildcards in the text are literal.
    assert!(queries::list_work_permits(&db, filter("%"))
        .await
        .expect("literal")
        .is_empty());
    assert_eq!(
        queries::list_work_permits(&db, filter("Hot work"))
            .await
            .expect("type")
            .len(),
        1
    );
}

fn type_input(
    t: &crate::permit::domain::PermitType,
    requires: Option<bool>,
    reason: Option<&str>,
) -> crate::permit::domain::PermitTypeUpsertInput {
    crate::permit::domain::PermitTypeUpsertInput {
        id: Some(t.id),
        code: t.code.clone(),
        name: t.name.clone(),
        description: Some(t.description.clone()),
        requires_hse_approval: t.requires_hse_approval,
        requires_operations_approval: t.requires_operations_approval,
        requires_atmospheric_test: t.requires_atmospheric_test,
        max_duration_hours: t.max_duration_hours,
        mandatory_ppe_ids_json: None,
        mandatory_control_rules_json: None,
        requires_energy_isolation: requires,
        disable_isolation_reason: reason.map(str::to_string),
    }
}

#[tokio::test]
async fn switching_the_isolation_rule_off_is_guarded_and_loto_is_locked() {
    let db = setup().await;
    let types = queries::list_permit_types(&db).await.expect("types");
    let loto = types.iter().find(|t| t.code == "loto").expect("loto").clone();
    let hot = types.iter().find(|t| t.code == "hot_work").expect("hot").clone();

    // Turning it on: a plain edit.
    let hot = queries::upsert_permit_type(&db, type_input(&hot, Some(true), None), EDITOR)
        .await
        .expect("turn on");
    assert!(hot.requires_energy_isolation);

    // Off: refused without the administrator right + step-up, then without a reason.
    assert_validation(
        queries::upsert_permit_type(&db, type_input(&hot, Some(false), Some("x")), EDITOR).await,
        "PERMIT_TYPE_ISOLATION_RELAX_FORBIDDEN",
    );
    assert_validation(
        queries::upsert_permit_type(&db, type_input(&hot, Some(false), Some("  ")), ADMIN_STEP_UP).await,
        "PERMIT_TYPE_ISOLATION_RELAX_REASON",
    );
    let before = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = 'permit_type.energy_isolation.disabled'",
    )
    .await;
    let off = queries::upsert_permit_type(
        &db,
        type_input(&hot, Some(false), Some("Hot work on isolated lines only")),
        ADMIN_STEP_UP,
    )
    .await
    .expect("administrator switches it off");
    assert!(!off.requires_energy_isolation);
    let after = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = 'permit_type.energy_isolation.disabled'",
    )
    .await;
    assert_eq!(after, before + 1, "audited");

    // The shipped LOTO type can never stop requiring an isolation.
    assert_validation(
        queries::upsert_permit_type(&db, type_input(&loto, Some(false), Some("reason")), ADMIN_STEP_UP).await,
        "PERMIT_TYPE_LOTO_ISOLATION_LOCKED",
    );
    let loto_now = queries::get_permit_type(&db, loto.id)
        .await
        .expect("read")
        .expect("loto");
    assert!(loto_now.requires_energy_isolation);
}

#[tokio::test]
async fn a_permit_always_has_a_bounded_future_expiry() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-70").await;
    let create = |expires: Option<String>, ty: i64| WorkPermitCreateInput {
        permit_type_id: ty,
        asset_id: Some(asset),
        entity_id: Some(1),
        linked_work_order_id: None,
        expires_at: expires,
    };
    let hot = type_id(&db, "hot_work").await; // max 8 h
    assert_validation(
        queries::create_work_permit(&db, create(None, hot)).await,
        "PERMIT_EXPIRY_REQUIRED",
    );
    assert_validation(
        queries::create_work_permit(&db, create(Some("  ".into()), hot)).await,
        "PERMIT_EXPIRY_REQUIRED",
    );
    let too_long = (chrono::Utc::now() + chrono::Duration::hours(10)).to_rfc3339();
    assert_validation(
        queries::create_work_permit(&db, create(Some(too_long.clone()), hot)).await,
        "PERMIT_EXPIRY_TOO_LONG",
    );
    let wp = queries::create_work_permit(
        &db,
        create(
            Some((chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339()),
            hot,
        ),
    )
    .await
    .expect("within the type's duration");
    // An update can't push it past the type's maximum either.
    assert_validation(
        queries::update_work_permit(
            &db,
            WorkPermitUpdateInput {
                id: wp.id,
                linked_work_order_id: None,
                asset_id: None,
                entity_id: None,
                expires_at: Some(too_long),
                expected_row_version: wp.row_version,
            },
        )
        .await,
        "PERMIT_EXPIRY_TOO_LONG",
    );

    // A permit stored without expiry (created before the rule) cannot be activated.
    let wp = advance_to_issued(&db, wp).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET expires_at = NULL WHERE id = ?",
        [wp.id.into()],
    ))
    .await
    .expect("legacy permit without expiry");
    let wp = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    assert_validation(set_status(&db, &wp, "active").await, "PERMIT_EXPIRY_MISSING");
}

/// Editing a permit cannot link it to a work order that already has an open permit (same rule
/// as creation); refusals of the permit rules are coded.
#[tokio::test]
async fn editing_a_permit_cannot_link_a_second_open_permit_to_a_work_order() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-80").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let first = create_from_wo(&db, "loto", wo_id).await.expect("first permit");
    let other = create_permit(&db, "hot_work", asset).await;
    let res = queries::update_work_permit(
        &db,
        WorkPermitUpdateInput {
            id: other.id,
            linked_work_order_id: Some(wo_id),
            asset_id: None,
            entity_id: None,
            expires_at: None,
            expected_row_version: other.row_version,
        },
    )
    .await;
    assert_validation(res, "PERMIT_WO_ALREADY_HAS_OPEN");

    // The permit already on the work order can still be edited (it is not "another" permit).
    queries::update_work_permit(
        &db,
        WorkPermitUpdateInput {
            id: first.id,
            linked_work_order_id: Some(wo_id),
            asset_id: None,
            entity_id: None,
            expires_at: None,
            expected_row_version: first.row_version,
        },
    )
    .await
    .expect("same permit, same work order");

    // Lifecycle refusals carry their code.
    assert_validation(set_status(&db, &other, "draft").await, "PERMIT_STATUS_UNCHANGED");
    assert_validation(set_status(&db, &other, "active").await, "PERMIT_TRANSITION_NOT_ALLOWED");
}
