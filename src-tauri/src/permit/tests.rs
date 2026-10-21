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
    may_change_three_person: false,
};
/// An administrator with a valid step-up (checked by the command).
const ADMIN_STEP_UP: queries::PermitTypeWriter = queries::PermitTypeWriter {
    actor_id: APPLIER,
    may_relax_isolation: true,
    may_change_three_person: true,
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
        APPLIER,
    )
    .await
    .expect("create permit")
}

/// Moves a permit to `status` as APPLIER, who wrote and submitted it; the approval is made by a
/// second account (four-eyes: the approver is neither the author nor the submitter).
pub(super) async fn set_status(db: &DatabaseConnection, wp: &WorkPermit, status: &str) -> Result<WorkPermit, AppError> {
    let actor = if status == "approved" {
        second_approver(db).await
    } else {
        APPLIER
    };
    set_status_as(db, wp, status, actor).await
}

pub(super) async fn set_status_as(
    db: &DatabaseConnection,
    wp: &WorkPermit,
    status: &str,
    actor: i64,
) -> Result<WorkPermit, AppError> {
    queries::set_work_permit_status(
        db,
        WorkPermitStatusInput {
            id: wp.id,
            status: status.into(),
            expected_row_version: wp.row_version,
        },
        actor,
    )
    .await
}

/// A second account approving permits in tests that sign everything else as the seeded admin
/// (four-eyes: the approver is neither the author nor the submitter).
pub(super) async fn second_approver(db: &DatabaseConnection) -> i64 {
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, \
         is_admin, force_password_change, created_at, updated_at) VALUES ('user-sync-ptw-approver', 'ptw_approver', \
         'PTW Approver', 'local', 1, 0, 0, datetime('now'), datetime('now'))"
            .to_string(),
    ))
    .await
    .expect("insert approver account");
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'ptw_approver'").await
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

/// "J'ai retiré mon cadenas": the lock's applier takes it off (before its removal is verified).
pub(super) async fn take_off(db: &DatabaseConnection, iso: &PermitIsolation) -> PermitIsolation {
    queries::record_permit_isolation_removed(db, action(iso), iso.applied_by_id.expect("applied"))
        .await
        .expect("lock taken off by its applier")
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
        APPLIER,
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
    add_handover_role(&db, "Maintenance").await;
    add_handover_role(&db, "Operations").await;
    queries::append_permit_handover_log(
        &db,
        PermitHandoverLogInput {
            permit_id: wp.id,
            handed_from_role: "Maintenance".into(),
            handed_to_role: "Operations".into(),
            confirmation_note: "Area clean".into(),
            received_by_id: None,
        },
        APPLIER,
    )
    .await
    .expect("handover log");
    let wp = set_status(&db, &wp, "handed_back").await.expect("hand back");
    assert!(wp.handed_back_at.is_some());
}

/// Gives `user_id` a plant-wide role carrying `permission` (a hand-over receiver holds
/// `ptw.close` at the permit's site: CMMS Advisor decision, option A).
pub(super) async fn grant_tenant_right(db: &DatabaseConnection, user_id: i64, permission: &str) {
    let role = crate::auth::scope_tests::role(db, &format!("PTW test {permission} {user_id}"), &[permission]).await;
    crate::auth::scope_tests::assign(db, i32::try_from(user_id).expect("id"), role, "tenant", None).await;
}

/// A role of the plant's hand-over list (`PERMIT.HANDOVER_ROLES`), as Données de référence adds it.
pub(super) async fn add_handover_role(db: &DatabaseConnection, code: &str) {
    crate::permit::handover_roles::ensure_handover_roles(db)
        .await
        .expect("list");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) \
         SELECT rs.id, ?, ?, 1, 1 FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'PERMIT.HANDOVER_ROLES' AND rs.status = 'published' \
            AND NOT EXISTS (SELECT 1 FROM reference_values v WHERE v.set_id = rs.id AND v.code = ?) \
          ORDER BY rs.version_no DESC LIMIT 1",
        [code.into(), code.into(), code.into()],
    ))
    .await
    .expect("role");
}

fn refusal_code(err: AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

/// A hand-over is signed by the session user (never from the screen), names roles of the plant's
/// list and a note of 10 characters or more; the receiving person, when named, is another active
/// account.
#[tokio::test]
async fn a_handover_is_signed_from_the_session_with_listed_roles_and_a_note() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-HO").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let wp = set_status(&db, &wp, "closed").await.expect("close");
    let input = |from: &str, to: &str, note: &str, receiver: Option<i64>| PermitHandoverLogInput {
        permit_id: wp.id,
        handed_from_role: from.into(),
        handed_to_role: to.into(),
        confirmation_note: note.into(),
        received_by_id: receiver,
    };

    // A role that is not in the list is refused (the list starts with the three starter roles).
    let err = queries::append_permit_handover_log(
        &db,
        input("Maintenance", "Operations", "Zone left clean", None),
        APPLIER,
    )
    .await
    .expect_err("Operations is not in the list");
    assert_eq!(refusal_code(err), crate::permit::handover_roles::CODE_ROLE_NOT_IN_LIST);

    add_handover_role(&db, "Maintenance").await;
    add_handover_role(&db, "Operations").await;
    let err = queries::append_permit_handover_log(&db, input("Maintenance", "Operations", "ok", None), APPLIER)
        .await
        .expect_err("short note");
    assert_eq!(refusal_code(err), "PERMIT_HANDOVER_NOTE_TOO_SHORT");
    let err = queries::append_permit_handover_log(
        &db,
        input("Maintenance", "Operations", "Zone left clean", Some(APPLIER)),
        APPLIER,
    )
    .await
    .expect_err("receiver is signer");
    assert_eq!(refusal_code(err), "PERMIT_HANDOVER_RECEIVER_IS_SIGNER");
    let err = queries::append_permit_handover_log(
        &db,
        input("Maintenance", "Operations", "Zone left clean", Some(999_999)),
        APPLIER,
    )
    .await
    .expect_err("unknown receiver");
    assert_eq!(refusal_code(err), "PERMIT_HANDOVER_RECEIVER_UNKNOWN");
    // The receiver holds the permit close right at the permit's site (never left to the picker).
    let err = queries::append_permit_handover_log(
        &db,
        input("Maintenance", "Operations", "Zone left clean", Some(VERIFIER)),
        APPLIER,
    )
    .await
    .expect_err("receiver without ptw.close");
    assert_eq!(refusal_code(err), "PERMIT_HANDOVER_RECEIVER_NO_RIGHT");
    grant_tenant_right(&db, VERIFIER, crate::rbac::permissions::PTW_CLOSE).await;

    let log = queries::append_permit_handover_log(
        &db,
        input("maintenance", "Operations", "  Zone left clean  ", Some(VERIFIER)),
        APPLIER,
    )
    .await
    .expect("handover");
    assert_eq!(log.handed_from_role, "Maintenance", "stored as the list code");
    assert_eq!(log.confirmation_note, "Zone left clean");
    assert_eq!(log.signed_by_id, Some(APPLIER));
    assert!(log.signed_by_name.is_some());
    assert_eq!(log.received_by_id, Some(VERIFIER));
    let listed = queries::list_permit_handover_logs(&db, wp.id).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].id, listed[0].signed_by_id, listed[0].received_by_id),
        (log.id, Some(APPLIER), Some(VERIFIER))
    );
}

/// The receiving person must be an active, non-deleted account; a signer or receiver whose display
/// name is empty or blank is shown on the stored hand-over under the login name, as in the receiver
/// list (never as nobody).
#[tokio::test]
async fn a_handover_refuses_inactive_receivers_and_never_shows_a_blank_name() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-HO3").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let wp = set_status(&db, &wp, "closed").await.expect("close");
    add_handover_role(&db, "Maintenance").await;
    add_handover_role(&db, "Operations").await;
    for (sync, user, name, active, deleted) in [
        ("ho3-inactive", "op.inactive", "Inactive Operator", 0, false),
        ("ho3-deleted", "op.deleted", "Deleted Operator", 1, true),
        ("ho3-empty", "op.empty", "", 1, false),
        ("ho3-blank", "op.blank", "   ", 1, false),
    ] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
             force_password_change, created_at, updated_at, deleted_at) VALUES (?, ?, ?, 'local', ?, 0, 0, \
             datetime('now'), datetime('now'), CASE WHEN ? THEN datetime('now') END)",
            [
                sync.into(),
                user.into(),
                name.into(),
                i64::from(active).into(),
                i64::from(deleted).into(),
            ],
        ))
        .await
        .expect("user");
    }
    let id_of = |user: &'static str| {
        let db = &db;
        async move { scalar_i64(db, &format!("SELECT id FROM user_accounts WHERE username = '{user}'")).await }
    };
    let input = |receiver: i64| PermitHandoverLogInput {
        permit_id: wp.id,
        handed_from_role: "Maintenance".into(),
        handed_to_role: "Operations".into(),
        confirmation_note: "Zone left clean".into(),
        received_by_id: Some(receiver),
    };

    for user in ["op.inactive", "op.deleted"] {
        let err = queries::append_permit_handover_log(&db, input(id_of(user).await), APPLIER)
            .await
            .expect_err("receiver not active");
        assert_eq!(refusal_code(err), "PERMIT_HANDOVER_RECEIVER_UNKNOWN", "{user}");
    }

    // Signed by an account with an empty display name, received by one with a blank one.
    grant_tenant_right(&db, id_of("op.blank").await, crate::rbac::permissions::PTW_CLOSE).await;
    let log = queries::append_permit_handover_log(&db, input(id_of("op.blank").await), id_of("op.empty").await)
        .await
        .expect("handover");
    assert_eq!(log.signed_by_name.as_deref(), Some("op.empty"));
    assert_eq!(log.received_by_name.as_deref(), Some("op.blank"));
    let listed = queries::list_permit_handover_logs(&db, wp.id).await.expect("list");
    assert_eq!(
        (
            listed[0].signed_by_name.as_deref(),
            listed[0].received_by_name.as_deref()
        ),
        (Some("op.empty"), Some("op.blank"))
    );
    // The receiver list shows the same name for the same person.
    let page = queries::list_handover_receivers(&db, APPLIER, wp.id, Some("op.blank".into()), None, None)
        .await
        .expect("receivers");
    assert_eq!(
        page.items.iter().map(|r| r.display_name.as_str()).collect::<Vec<_>>(),
        ["op.blank"]
    );
}

/// Free texts stored before the list existed are adopted with their exact text, flagged
/// "à revoir"; the stored hand-over and its (unknown) signer are left as they were.
#[tokio::test]
async fn stored_handover_roles_are_adopted_and_flagged_without_rewrite() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-HO2").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
         confirmation_note, signed_at, row_version) VALUES ('ho-old', ?, 'Équipe nuit', 'Exploitation', '', \
         '2026-01-01T00:00:00Z', 1)",
        [wp.id.into()],
    ))
    .await
    .expect("old log");
    crate::permit::handover_roles::adopt_stored_roles(&db)
        .await
        .expect("adopt");
    crate::permit::handover_roles::adopt_stored_roles(&db)
        .await
        .expect("again");
    let roles = crate::permit::handover_roles::list_handover_roles(&db)
        .await
        .expect("roles");
    let codes: Vec<_> = roles.iter().map(|r| (r.code.as_str(), r.review_required)).collect();
    // The starter roles (migration 239) first; "Exploitation" is one of them, so it is not added twice.
    assert_eq!(
        codes,
        [
            ("Maintenance", true),
            ("Exploitation", true),
            ("HSE", true),
            ("Équipe nuit", true)
        ]
    );
    let listed = queries::list_permit_handover_logs(&db, wp.id).await.expect("list");
    assert_eq!(listed[0].handed_from_role, "Équipe nuit");
    assert_eq!(listed[0].handed_from_label.as_deref(), Some("Équipe nuit"));
    assert_eq!(listed[0].signed_by_id, None, "no signer invented");
}

/// A stored hand-over shows its roles' labels from the plant's list even after a role is
/// deactivated or left out of a newer published version of the list; the newest published version
/// gives the label, a draft never does, and a code in no version has no label (shown as stored).
#[tokio::test]
async fn handover_role_labels_resolve_over_inactive_roles_and_older_versions() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-HO4").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let wp = set_status(&db, &wp, "closed").await.expect("close");
    add_handover_role(&db, "Operations").await;
    let exec = |sql: &'static str| {
        let db = &db;
        async move {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                .await
                .expect(sql);
        }
    };
    exec(
        "UPDATE reference_values SET label = 'Opérations' WHERE code = 'Operations' AND set_id IN (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
         WHERE d.code = 'PERMIT.HANDOVER_ROLES')",
    )
    .await;
    queries::append_permit_handover_log(
        &db,
        PermitHandoverLogInput {
            permit_id: wp.id,
            handed_from_role: "maintenance".into(),
            handed_to_role: "Operations".into(),
            confirmation_note: "Zone left clean".into(),
            received_by_id: None,
        },
        APPLIER,
    )
    .await
    .expect("handover");
    let labels =
        |log: &crate::permit::domain::PermitHandoverLog| (log.handed_from_label.clone(), log.handed_to_label.clone());
    let listed = queries::list_permit_handover_logs(&db, wp.id).await.expect("list");
    assert_eq!(
        labels(&listed[0]),
        (Some("Maintenance".into()), Some("Opérations".into()))
    );

    // "Operations" is deactivated, then a new version of the list is published without it and
    // with "Maintenance" renamed; a draft version renames "Operations" again.
    exec(
        "UPDATE reference_values SET is_active = 0 WHERE code = 'Operations' AND set_id IN (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
         WHERE d.code = 'PERMIT.HANDOVER_ROLES')",
    )
    .await;
    exec(
        "UPDATE reference_sets SET status = 'superseded' WHERE status = 'published' AND domain_id = \
         (SELECT id FROM reference_domains WHERE code = 'PERMIT.HANDOVER_ROLES')",
    )
    .await;
    exec(
        "INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_at, published_at) \
         SELECT id, 2, 'published', '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z' \
           FROM reference_domains WHERE code = 'PERMIT.HANDOVER_ROLES'",
    )
    .await;
    exec(
        "INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_at) \
         SELECT id, 3, 'draft', '2026-10-02T00:00:00Z', '2026-10-02T00:00:00Z' \
           FROM reference_domains WHERE code = 'PERMIT.HANDOVER_ROLES'",
    )
    .await;
    exec(
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) \
         SELECT rs.id, 'Maintenance', 'Maintenance mécanique', 1, 1 FROM reference_sets rs \
           JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'PERMIT.HANDOVER_ROLES' AND rs.version_no = 2",
    )
    .await;
    exec(
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) \
         SELECT rs.id, 'Operations', 'Brouillon', 1, 1 FROM reference_sets rs \
           JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'PERMIT.HANDOVER_ROLES' AND rs.version_no = 3",
    )
    .await;
    let roles = crate::permit::handover_roles::list_handover_roles(&db)
        .await
        .expect("roles");
    assert_eq!(
        roles.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(),
        ["Maintenance"],
        "the form lists only active roles of the current version"
    );

    // A role code that no version of the list holds (e.g. written by hand) has no label.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
         confirmation_note, signed_at, row_version) VALUES ('ho4-unknown', ?, 'Inconnu', 'Operations', \
         'Zone left clean', '2026-10-03T00:00:00Z', 1)",
        [wp.id.into()],
    ))
    .await
    .expect("log");

    let listed = queries::list_permit_handover_logs(&db, wp.id).await.expect("list");
    assert_eq!(listed.len(), 2);
    assert_eq!(
        labels(&listed[0]),
        (Some("Maintenance mécanique".into()), Some("Opérations".into())),
        "newest published version first; a role left out keeps its label; a draft is ignored"
    );
    assert_eq!(labels(&listed[1]), (None, Some("Opérations".into())));
}

/// A stored role whose exact code is in no version of the list (a free text in another case) gets
/// the label of the code that matches it ignoring case; a code whose exact match is held only by an
/// older version keeps that exact match's label, even when the newest version holds the same code
/// in another case.
#[tokio::test]
async fn handover_role_labels_fall_back_to_the_code_ignoring_case_only_without_an_exact_match() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-HO5").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let exec = |sql: String| {
        let db = &db;
        async move {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql.clone()))
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
    };
    // Written raw (an older version or another PC stored the text as typed): the hand-over form
    // would store the list's own code.
    exec(format!(
        "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
         confirmation_note, signed_at, row_version) VALUES ('ho5-lower', {}, 'exploitation', 'Chef', \
         'Zone left clean', '2026-10-03T00:00:00Z', 1)",
        wp.id
    ))
    .await;
    // "Chef" (exact) in the current version 1; then version 2 replaces it with "CHEF".
    exec(
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) \
         SELECT rs.id, 'Chef', 'Chef (ancien)', 5, 1 FROM reference_sets rs \
           JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE d.code = 'PERMIT.HANDOVER_ROLES' AND rs.status = 'published'"
            .to_string(),
    )
    .await;
    exec(
        "UPDATE reference_sets SET status = 'superseded' WHERE status = 'published' AND domain_id = \
         (SELECT id FROM reference_domains WHERE code = 'PERMIT.HANDOVER_ROLES')"
            .to_string(),
    )
    .await;
    exec(
        "INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_at, published_at) \
         SELECT d.id, (SELECT MAX(version_no) + 1 FROM reference_sets WHERE domain_id = d.id), 'published', \
                '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z' \
           FROM reference_domains d WHERE d.code = 'PERMIT.HANDOVER_ROLES'"
            .to_string(),
    )
    .await;
    exec(
        "INSERT INTO reference_values (set_id, code, label, sort_order, is_active) \
         SELECT rs.id, v.code, v.label, v.sort_order, 1 FROM reference_sets rs \
           JOIN reference_domains d ON d.id = rs.domain_id \
           JOIN (SELECT 'Exploitation' AS code, 'Exploitation (nouveau)' AS label, 2 AS sort_order \
                 UNION ALL SELECT 'CHEF', 'Chef (nouveau)', 5) v \
          WHERE d.code = 'PERMIT.HANDOVER_ROLES' AND rs.status = 'published'"
            .to_string(),
    )
    .await;
    let listed = queries::list_permit_handover_logs(&db, wp.id).await.expect("list");
    let log = listed.iter().find(|l| l.entity_sync_id == "ho5-lower").expect("log");
    assert_eq!(
        log.handed_from_label.as_deref(),
        Some("Exploitation (nouveau)"),
        "no exact match anywhere: the code ignoring case, from the newest version"
    );
    assert_eq!(
        log.handed_to_label.as_deref(),
        Some("Chef (ancien)"),
        "the exact match of an older version wins over another case in the newest one"
    );
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
        APPLIER,
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
    let iso = take_off(&db, &iso).await;
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
    let iso = take_off(&db, &iso).await;
    queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("removal");
    let wp = set_status(&db, &wp, "closed").await.expect("close after removal");
    assert_eq!(wp.status, "closed");
}

// ── The applier takes their own lock off (migration 249) ──────────────

/// "J'ai retiré mon cadenas": only the applier records it, once, while removal is open; the
/// removal of a lock fitted before this rule is not verified until it is recorded (no
/// grandfathering); both steps are audited.
#[tokio::test]
async fn only_the_applier_takes_the_lock_off_before_its_removal_is_verified() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-LR1").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-LR1").await).await;
    // Not while the permit is only issued.
    let err = queries::record_permit_isolation_removed(&db, action(&iso), APPLIER)
        .await
        .expect_err("issued");
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_WRONG_STATUS");
    let wp = set_status(&db, &wp, "active").await.expect("activate");

    // A lock already fitted: its removal is not verified before it is taken off.
    let err = queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect_err("not taken off");
    assert_eq!(refusal_code(err), "PERMIT_REMOVAL_LOCK_NOT_REMOVED");
    // Someone else cannot record it on the applier's behalf (that is a forced removal).
    let err = queries::record_permit_isolation_removed(&db, action(&iso), VERIFIER)
        .await
        .expect_err("not the applier");
    assert_eq!(refusal_code(err), "PERMIT_LOCK_REMOVAL_NOT_APPLIER");

    let removed = take_off(&db, &iso).await;
    assert_eq!(removed.removed_by_id, Some(APPLIER));
    assert!(removed.removed_at.is_some());
    assert!(!removed.forced_removal);
    assert!(removed.removal_verified_at.is_none(), "still to verify");
    let err = queries::record_permit_isolation_removed(&db, action(&removed), APPLIER)
        .await
        .expect_err("twice");
    assert_eq!(refusal_code(err), "PERMIT_LOCK_ALREADY_REMOVED");

    let verified = queries::verify_permit_isolation_removal(&db, action(&removed), VERIFIER)
        .await
        .expect("verified by a second person");
    assert_eq!(verified.removal_verified_by_id, Some(VERIFIER));
    assert_eq!(audit_count(&db, queries::AUDIT_LOCK_REMOVED).await, 1);
    assert_eq!(audit_count(&db, queries::AUDIT_LOCK_REMOVAL_VERIFIED).await, 1);
    set_status(&db, &wp, "closed").await.expect("close");
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
            // Planned in the past: the permit gate judges now + this duration (CMMS Advisor rule),
            // which the test permits (8 h from their creation) cover.
            expected_duration_hours: Some(1.0),
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
        APPLIER,
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
    let iso = take_off(&db, &iso).await;
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

    // Neither taken off by its applier nor verified while the work runs.
    assert_validation(
        queries::record_permit_isolation_removed(&db, action(&iso), APPLIER).await,
        "still in progress",
    );
    assert_validation(
        queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER).await,
        "PERMIT_REMOVAL_LOCK_NOT_REMOVED",
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
    let iso = take_off(&db, &iso).await;
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
            requires_three_person_removal: None,
            three_person_change_reason: None,
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
            requires_three_person_removal: None,
            three_person_change_reason: None,
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
        .expect("search")
        .items;
    assert_eq!(hits.iter().map(|p| p.id).collect::<Vec<_>>(), vec![pa.id]);
    // LIKE wildcards in the text are literal.
    assert!(queries::list_work_permits(&db, filter("%"))
        .await
        .expect("literal")
        .items
        .is_empty());
    assert_eq!(
        queries::list_work_permits(&db, filter("Hot work"))
            .await
            .expect("type")
            .items
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
        requires_three_person_removal: None,
        three_person_change_reason: None,
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
    // Shorter than the minimum (counted after trimming): refused, naming the minimum.
    let err = queries::upsert_permit_type(&db, type_input(&hot, Some(false), Some("  too short ")), ADMIN_STEP_UP)
        .await
        .expect_err("short reason");
    match err {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues[0].code, "PERMIT_TYPE_ISOLATION_RELAX_REASON");
            assert_eq!(
                issues[0].params.get("min"),
                Some(&crate::settings::catalog::MIN_REASON_CHARS.to_string())
            );
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
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
    // The type records who switched it off, when and why (sent with it, re-checked on arrival).
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM permit_types WHERE id = {} AND isolation_changed_by_id = {APPLIER} \
                 AND isolation_changed_at IS NOT NULL \
                 AND isolation_change_reason = 'Hot work on isolated lines only'",
                hot.id
            ),
        )
        .await,
        1
    );
    // Turning it on again is a plain edit and keeps that record.
    let on_again = queries::upsert_permit_type(&db, type_input(&off, Some(true), None), EDITOR)
        .await
        .expect("on again");
    assert!(on_again.requires_energy_isolation);
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM permit_types WHERE id = {} AND isolation_changed_by_id = {APPLIER}",
                hot.id
            ),
        )
        .await,
        1
    );
    // Off again while the recorded switch-off is later than this PC's time (its clock is behind):
    // refused, naming the time of the last change.
    let ahead = (chrono::Utc::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE permit_types SET isolation_changed_at = ? WHERE id = ?",
        [ahead.into(), hot.id.into()],
    ))
    .await
    .expect("record ahead");
    assert_validation(
        queries::upsert_permit_type(
            &db,
            type_input(&on_again, Some(false), Some("Hot work on isolated lines only")),
            ADMIN_STEP_UP,
        )
        .await,
        "PERMIT_TYPE_ISOLATION_CLOCK_BEHIND",
    );

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
        queries::create_work_permit(&db, create(None, hot), APPLIER).await,
        "PERMIT_EXPIRY_REQUIRED",
    );
    assert_validation(
        queries::create_work_permit(&db, create(Some("  ".into()), hot), APPLIER).await,
        "PERMIT_EXPIRY_REQUIRED",
    );
    let too_long = (chrono::Utc::now() + chrono::Duration::hours(10)).to_rfc3339();
    assert_validation(
        queries::create_work_permit(&db, create(Some(too_long.clone()), hot), APPLIER).await,
        "PERMIT_EXPIRY_TOO_LONG",
    );
    let wp = queries::create_work_permit(
        &db,
        create(
            Some((chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339()),
            hot,
        ),
        APPLIER,
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
            APPLIER,
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
        APPLIER,
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
        APPLIER,
    )
    .await
    .expect("same permit, same work order");

    // Lifecycle refusals carry their code.
    assert_validation(set_status(&db, &other, "draft").await, "PERMIT_STATUS_UNCHANGED");
    assert_validation(set_status(&db, &other, "active").await, "PERMIT_TRANSITION_NOT_ALLOWED");
}

/// Hand-over receivers: active accounts holding the permit close right at the permit's site
/// (here plant-wide), never the person asking; names only; paged with the real total. Asking
/// without the right is refused by the command (`permit::access`, `access_tests`).
#[tokio::test]
async fn handover_receivers_are_other_active_accounts_by_name() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-RCV").await;
    let wp = create_permit(&db, "hot_work", asset).await;
    for (sync, user, name, active) in [
        ("rcv-1", "op.ali", "Ali Operations", 1),
        ("rcv-2", "op.bea", "Béa Operations", 1),
        ("rcv-3", "op.old", "Old Operator", 0),
        ("rcv-4", "noname", "", 1),
        ("rcv-5", "op.none", "Nora Operations", 1),
    ] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
             force_password_change, created_at, updated_at) VALUES (?, ?, ?, 'local', ?, 0, 0, datetime('now'), datetime('now'))",
            [sync.into(), user.into(), name.into(), i64::from(active).into()],
        ))
        .await
        .expect("user");
    }
    let ali: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'op.ali'".to_string(),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index(0)
        .expect("id");

    // Every one of them holds the permit close right except Nora.
    for user in ["op.ali", "op.bea", "op.old", "noname"] {
        let id = scalar_i64(&db, &format!("SELECT id FROM user_accounts WHERE username = '{user}'")).await;
        grant_tenant_right(&db, id, crate::rbac::permissions::PTW_CLOSE).await;
    }
    assert!(ali > 0);

    // The administrator (APPLIER) asks: every active holder of the right but himself.
    let page = queries::list_handover_receivers(&db, APPLIER, wp.id, None, None, None)
        .await
        .expect("list");
    let names: Vec<&str> = page.items.iter().map(|r| r.display_name.as_str()).collect();
    assert!(page.items.iter().all(|r| r.id != APPLIER), "the signer is excluded");
    assert!(names.contains(&"Ali Operations") && names.contains(&"Béa Operations"));
    assert!(
        !names.contains(&"Nora Operations"),
        "without the permit close right: not a receiver"
    );
    assert_eq!(page.empty_reason, None);
    assert!(!names.contains(&"Old Operator"), "inactive accounts are excluded");
    assert!(names.contains(&"noname"), "no display name: the login name stands in");
    assert_eq!(page.total, i64::try_from(page.items.len()).expect("len"));

    let first = queries::list_handover_receivers(&db, APPLIER, wp.id, Some("operations".into()), Some(1), None)
        .await
        .expect("page 1");
    assert_eq!(first.total, 2, "the total counts every match, not the page");
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].display_name, "Ali Operations");
    let second = queries::list_handover_receivers(&db, APPLIER, wp.id, Some("operations".into()), Some(1), Some(1))
        .await
        .expect("page 2");
    assert_eq!(second.items[0].display_name, "Béa Operations");
}

// ── Three-person LOTO chain, a setting of each permit type (migration 236) ──

const CHAIN_ON: &str = "permit_type.three_person_removal.enabled";
const CHAIN_OFF: &str = "permit_type.three_person_removal.disabled";
const GOOD_REASON: &str = "Site rule after the 2026 LOTO audit";

async fn audit_count(db: &DatabaseConnection, action_code: &str) -> i64 {
    scalar_i64(
        db,
        &format!("SELECT COUNT(*) FROM audit_events WHERE action_code = '{action_code}'"),
    )
    .await
}

fn chain_input(
    t: &crate::permit::domain::PermitType,
    chain: Option<bool>,
    reason: Option<&str>,
) -> crate::permit::domain::PermitTypeUpsertInput {
    let mut input = type_input(t, None, None);
    input.requires_three_person_removal = chain;
    input.three_person_change_reason = reason.map(str::to_string);
    input
}

fn new_type_input(
    code: &str,
    chain: Option<bool>,
    reason: Option<&str>,
) -> crate::permit::domain::PermitTypeUpsertInput {
    crate::permit::domain::PermitTypeUpsertInput {
        id: None,
        code: code.into(),
        name: format!("Type {code}"),
        description: None,
        requires_hse_approval: false,
        requires_operations_approval: false,
        requires_atmospheric_test: false,
        max_duration_hours: None,
        mandatory_ppe_ids_json: None,
        mandatory_control_rules_json: None,
        requires_energy_isolation: None,
        disable_isolation_reason: None,
        requires_three_person_removal: chain,
        three_person_change_reason: reason.map(str::to_string),
    }
}

#[tokio::test]
async fn the_three_person_chain_is_off_on_every_existing_type() {
    let db = setup().await;
    let types = queries::list_permit_types(&db).await.expect("types");
    assert!(!types.is_empty());
    assert!(types.iter().all(|t| !t.requires_three_person_removal), "{types:?}");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM permit_types WHERE requires_three_person_removal <> 0"
        )
        .await,
        0
    );
    // NOT NULL DEFAULT 0, and only 0 / 1.
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM pragma_table_info('permit_types') \
             WHERE name = 'requires_three_person_removal' AND \"notnull\" = 1 AND dflt_value = '0'"
        )
        .await,
        1
    );
    let bad = db
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE permit_types SET requires_three_person_removal = 2 WHERE code = 'loto'".to_string(),
        ))
        .await;
    assert!(bad.is_err(), "the CHECK refuses a value other than 0 / 1");
}

#[tokio::test]
async fn creating_a_type_with_the_three_person_chain_needs_an_administrator_a_reason_and_is_audited() {
    let db = setup().await;
    let before = audit_count(&db, CHAIN_ON).await;

    let err = queries::upsert_permit_type(&db, new_type_input("chain_a", Some(true), Some(GOOD_REASON)), EDITOR)
        .await
        .expect_err("no adm.settings / step-up");
    assert_eq!(refusal_code(err), "PERMIT_TYPE_THREE_PERSON_FORBIDDEN");
    for reason in [None, Some("   "), Some("too short")] {
        let err = queries::upsert_permit_type(&db, new_type_input("chain_a", Some(true), reason), ADMIN_STEP_UP)
            .await
            .expect_err("reason");
        assert_eq!(refusal_code(err), "PERMIT_TYPE_THREE_PERSON_REASON", "{reason:?}");
    }
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM permit_types WHERE code = 'chain_a'").await,
        0,
        "nothing written on a refusal"
    );

    let created = queries::upsert_permit_type(
        &db,
        new_type_input("chain_a", Some(true), Some(GOOD_REASON)),
        ADMIN_STEP_UP,
    )
    .await
    .expect("administrator creates it with the chain");
    assert!(created.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_ON).await, before + 1);
    let details: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT details_json FROM audit_events WHERE action_code = ? AND target_id = ? \
             AND auth_context = 'step_up' AND actor_id = ?",
            [CHAIN_ON.into(), created.id.to_string().into(), APPLIER.into()],
        ))
        .await
        .expect("q")
        .expect("audit row of the new type")
        .try_get("", "details_json")
        .expect("details");
    let details: serde_json::Value = serde_json::from_str(&details).expect("json");
    assert_eq!(details["reason"], GOOD_REASON);
    assert_eq!(details["after"], true);
    assert!(details["before"].is_null(), "a new type had no setting before");

    // Omitted or off on a new type: the default, no guard, nothing audited.
    let plain = queries::upsert_permit_type(&db, new_type_input("chain_b", None, None), EDITOR)
        .await
        .expect("omitted");
    assert!(!plain.requires_three_person_removal);
    let off = queries::upsert_permit_type(&db, new_type_input("chain_c", Some(false), None), EDITOR)
        .await
        .expect("explicitly off");
    assert!(!off.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_ON).await, before + 1);
    assert_eq!(audit_count(&db, CHAIN_OFF).await, 0);
}

#[tokio::test]
async fn changing_the_three_person_chain_is_guarded_both_ways_and_an_unchanged_value_is_not() {
    let db = setup().await;
    let types = queries::list_permit_types(&db).await.expect("types");
    let hot = types.iter().find(|t| t.code == "hot_work").expect("hot").clone();

    // On: refused without the administrator right + step-up, then without a long enough reason.
    let err = queries::upsert_permit_type(&db, chain_input(&hot, Some(true), Some(GOOD_REASON)), EDITOR)
        .await
        .expect_err("editor");
    assert_eq!(refusal_code(err), "PERMIT_TYPE_THREE_PERSON_FORBIDDEN");
    let err = queries::upsert_permit_type(&db, chain_input(&hot, Some(true), Some("short")), ADMIN_STEP_UP)
        .await
        .expect_err("short reason");
    assert_eq!(refusal_code(err), "PERMIT_TYPE_THREE_PERSON_REASON");
    let hot_now = queries::get_permit_type(&db, hot.id).await.expect("read").expect("hot");
    assert!(!hot_now.requires_three_person_removal);
    assert_eq!(hot_now.row_version, hot.row_version, "nothing written on a refusal");

    let on = queries::upsert_permit_type(&db, chain_input(&hot, Some(true), Some(GOOD_REASON)), ADMIN_STEP_UP)
        .await
        .expect("administrator turns it on");
    assert!(on.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_ON).await, 1);

    // Omitted: unchanged, no guard. Same value: no change, no guard. Neither is audited.
    let renamed = queries::upsert_permit_type(&db, chain_input(&on, None, None), EDITOR)
        .await
        .expect("omitted");
    assert!(
        renamed.requires_three_person_removal,
        "an update without the field keeps it"
    );
    let same = queries::upsert_permit_type(&db, chain_input(&renamed, Some(true), None), EDITOR)
        .await
        .expect("same value");
    assert!(same.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_ON).await, 1);

    // Off: same guards.
    let err = queries::upsert_permit_type(&db, chain_input(&same, Some(false), Some(GOOD_REASON)), EDITOR)
        .await
        .expect_err("editor");
    assert_eq!(refusal_code(err), "PERMIT_TYPE_THREE_PERSON_FORBIDDEN");
    let err = queries::upsert_permit_type(&db, chain_input(&same, Some(false), None), ADMIN_STEP_UP)
        .await
        .expect_err("no reason");
    assert_eq!(refusal_code(err), "PERMIT_TYPE_THREE_PERSON_REASON");
    // A later change (the one above was recorded within this second: a change in the same second
    // is refused, `a_three_person_change_is_refused_when_this_pcs_clock_is_behind_the_last_change`).
    backdate_three_person_change(&db, same.id).await;
    let off = queries::upsert_permit_type(&db, chain_input(&same, Some(false), Some(GOOD_REASON)), ADMIN_STEP_UP)
        .await
        .expect("administrator turns it off");
    assert!(!off.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_OFF).await, 1);
    let same_off = queries::upsert_permit_type(&db, chain_input(&off, Some(false), None), EDITOR)
        .await
        .expect("same value (off)");
    assert!(!same_off.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_OFF).await, 1);
}

/// The type row itself says who last changed the three-person setting, when and why (migration
/// 238, sent with the type and re-checked on arrival); a save that does not change it keeps them.
#[tokio::test]
async fn a_change_of_the_three_person_chain_is_recorded_on_the_type() {
    let db = setup().await;
    let types = queries::list_permit_types(&db).await.expect("types");
    let hot = types.iter().find(|t| t.code == "hot_work").expect("hot").clone();
    let trace = |id: i64| {
        let db = &db;
        async move {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT three_person_changed_by_id AS by_id, three_person_changed_at AS at, \
                     three_person_change_reason AS reason FROM permit_types WHERE id = ?",
                    [id.into()],
                ))
                .await
                .expect("q")
                .expect("row");
            (
                row.try_get::<Option<i64>>("", "by_id").expect("by"),
                row.try_get::<Option<String>>("", "at").expect("at"),
                row.try_get::<Option<String>>("", "reason").expect("reason"),
            )
        }
    };
    assert_eq!(
        trace(hot.id).await,
        (None, None, None),
        "never changed: nobody invented"
    );

    let padded = format!("  {GOOD_REASON}  ");
    let on = queries::upsert_permit_type(&db, chain_input(&hot, Some(true), Some(&padded)), ADMIN_STEP_UP)
        .await
        .expect("on");
    let (by, at, reason) = trace(on.id).await;
    assert_eq!(by, Some(APPLIER));
    assert!(
        at.as_deref().and_then(crate::settings::site::instant).is_some(),
        "{at:?}"
    );
    assert_eq!(reason.as_deref(), Some(GOOD_REASON), "stored trimmed");

    let renamed = queries::upsert_permit_type(&db, chain_input(&on, None, None), EDITOR)
        .await
        .expect("plain edit");
    assert_eq!(
        trace(renamed.id).await,
        (by, at, reason),
        "kept by a save that does not change it"
    );

    let created = queries::upsert_permit_type(
        &db,
        new_type_input("chain_t", Some(true), Some(GOOD_REASON)),
        ADMIN_STEP_UP,
    )
    .await
    .expect("new type with the chain");
    let (by, _, reason) = trace(created.id).await;
    assert_eq!((by, reason.as_deref()), (Some(APPLIER), Some(GOOD_REASON)));
    let plain = queries::upsert_permit_type(&db, new_type_input("chain_u", None, None), EDITOR)
        .await
        .expect("new type without it");
    assert_eq!(trace(plain.id).await, (None, None, None));
}

/// A plant-wide role holding `permissions`, granted to a new active account (sync id
/// `<username>-sync`).
pub(super) async fn user_with(db: &DatabaseConnection, username: &str, permissions: &[&str]) -> i64 {
    for (sql, values) in [
        (
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
             force_password_change, created_at, updated_at) VALUES (?, ?, ?, 'local', 1, 0, 0, datetime('now'), datetime('now'))",
            vec![format!("{username}-sync").into(), username.into(), username.into()],
        ),
        (
            "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at) \
             VALUES (?, ?, '', 0, 'custom', 'active', datetime('now'), datetime('now'))",
            vec![format!("{username}-role").into(), format!("{username}-role").into()],
        ),
    ] {
        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await
            .expect("insert");
    }
    let user = scalar_i64(
        db,
        &format!("SELECT id FROM user_accounts WHERE username = '{username}'"),
    )
    .await;
    let role = scalar_i64(db, &format!("SELECT id FROM roles WHERE name = '{username}-role'")).await;
    for p in permissions {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) \
             SELECT ?, id, datetime('now') FROM permissions WHERE name = ?",
            [role.into(), (*p).into()],
        ))
        .await
        .expect("grant");
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, created_at, updated_at) \
         VALUES (?, ?, ?, 'tenant', datetime('now'), datetime('now'))",
        [format!("{username}-usa").into(), user.into(), role.into()],
    ))
    .await
    .expect("assign");
    user
}

fn session_user(id: i64, username: &str, is_admin: bool) -> crate::auth::session_manager::AuthenticatedUser {
    crate::auth::session_manager::AuthenticatedUser {
        user_id: i32::try_from(id).expect("fits"),
        username: username.into(),
        display_name: None,
        is_admin,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    }
}

/// The command asks `adm.settings` and a step-up only when the setting changes.
#[tokio::test]
async fn the_command_asks_adm_settings_and_step_up_to_change_the_three_person_chain() {
    let db = setup().await;
    let hot = queries::list_permit_types(&db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == "hot_work")
        .expect("hot");

    // A permit issuer without adm.settings: a plain edit passes, the chain change is refused.
    let issuer_id = user_with(&db, "ptw_issuer_only", &[crate::rbac::permissions::PTW_ISSUE]).await;
    let issuer = session_user(issuer_id, "ptw_issuer_only", false);
    let state = crate::state::AppState::new(db.clone());
    state.session.write().await.create_session(issuer.clone());
    let renamed = crate::commands::permit::upsert_permit_type_impl(&state, &issuer, chain_input(&hot, None, None))
        .await
        .expect("plain edit without adm.settings");
    let err = crate::commands::permit::upsert_permit_type_impl(
        &state,
        &issuer,
        chain_input(&renamed, Some(true), Some(GOOD_REASON)),
    )
    .await
    .expect_err("no adm.settings");
    assert!(matches!(err, AppError::PermissionDenied(_)), "{err:?}");

    // The administrator: refused until the step-up, then accepted (and audited).
    let admin = session_user(APPLIER, "admin", true);
    let state = crate::state::AppState::new(db.clone());
    state.session.write().await.create_session(admin.clone());
    let err = crate::commands::permit::upsert_permit_type_impl(
        &state,
        &admin,
        chain_input(&renamed, Some(true), Some(GOOD_REASON)),
    )
    .await
    .expect_err("no step-up");
    assert!(matches!(err, AppError::StepUpRequired), "{err:?}");
    let err = crate::commands::permit::upsert_permit_type_impl(
        &state,
        &admin,
        new_type_input("chain_cmd", Some(true), Some(GOOD_REASON)),
    )
    .await
    .expect_err("a new type with the chain: no step-up");
    assert!(matches!(err, AppError::StepUpRequired), "{err:?}");
    state.session.write().await.record_step_up();
    let on = crate::commands::permit::upsert_permit_type_impl(
        &state,
        &admin,
        chain_input(&renamed, Some(true), Some(GOOD_REASON)),
    )
    .await
    .expect("administrator with step-up");
    assert!(on.requires_three_person_removal);
    assert_eq!(audit_count(&db, CHAIN_ON).await, 1);
}

/// Turns the three-person chain on for the LOTO type (administrator, step-up, reason).
pub(super) async fn loto_with_three_person_chain(db: &DatabaseConnection) {
    let loto = queries::list_permit_types(db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == "loto")
        .expect("loto");
    let on = queries::upsert_permit_type(db, chain_input(&loto, Some(true), Some(GOOD_REASON)), ADMIN_STEP_UP)
        .await
        .expect("chain on");
    assert!(on.requires_three_person_removal);
}

#[tokio::test]
async fn with_the_three_person_chain_the_isolation_verifier_cannot_verify_the_removal() {
    let db = setup().await;
    loto_with_three_person_chain(&db).await;
    let third = user_with(&db, "ptw_third_person", &[]).await;
    let (wp, iso) = active_loto_permit(&db, "EQ-PTW-90").await;
    assert!(wp.permit_type_requires_three_person_removal);
    assert_eq!(iso.verified_by_id, Some(VERIFIER));
    assert_eq!(
        iso.verified_by_name.as_deref(),
        Some("PTW Verifier"),
        "the dialog can name who may not confirm"
    );

    // The lock is taken off by its applier: the three-person chain still applies to the verifier.
    let iso = take_off(&db, &iso).await;
    let err = queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect_err("verifier");
    assert_eq!(refusal_code(err), "PERMIT_THREE_PERSON_REMOVAL");
    // The two-person rule stays.
    let err = queries::verify_permit_isolation_removal(&db, action(&iso), APPLIER)
        .await
        .expect_err("applier");
    assert_eq!(refusal_code(err), "PERMIT_TWO_PERSON_REMOVAL");

    let removed = queries::verify_permit_isolation_removal(&db, action(&iso), third)
        .await
        .expect("a third person verifies the removal");
    assert_eq!(removed.removal_verified_by_id, Some(third));
    assert!(removed.removal_verified_at.is_some());
}

#[tokio::test]
async fn without_the_three_person_chain_the_verifier_may_verify_the_removal() {
    let db = setup().await;
    let (wp, iso) = active_loto_permit(&db, "EQ-PTW-91").await;
    assert!(!wp.permit_type_requires_three_person_removal, "off by default");
    let iso = take_off(&db, &iso).await;
    let err = queries::verify_permit_isolation_removal(&db, action(&iso), APPLIER)
        .await
        .expect_err("applier");
    assert_eq!(refusal_code(err), "PERMIT_TWO_PERSON_REMOVAL");
    let removed = queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("two-person rule only");
    assert_eq!(removed.removal_verified_by_id, Some(VERIFIER));
}

/// A three-person change is recorded at this PC's time; when the change already recorded for the
/// type is not earlier (this PC's clock is behind), it is refused here with the time to wait for,
/// instead of being shown as made here and refused on every other PC. An unreadable recorded time
/// is replaced by the new record.
#[tokio::test]
async fn a_three_person_change_is_refused_when_this_pcs_clock_is_behind_the_last_change() {
    let db = setup().await;
    let hot = queries::list_permit_types(&db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == "hot_work")
        .expect("hot_work");
    let ahead = (chrono::Utc::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let set_held = |at: &str| {
        let db = &db;
        let sql = format!(
            "UPDATE permit_types SET three_person_changed_at = '{at}', three_person_changed_by_id = {APPLIER}, \
             three_person_change_reason = 'Changed on a PC ahead in time' WHERE id = {}",
            hot.id
        );
        async move {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql))
                .await
                .expect("held record");
        }
    };
    set_held(&ahead).await;
    let err = queries::upsert_permit_type(&db, chain_input(&hot, Some(true), Some(GOOD_REASON)), ADMIN_STEP_UP)
        .await
        .expect_err("clock behind");
    match err {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues[0].code, "PERMIT_TYPE_THREE_PERSON_CLOCK_BEHIND");
            assert_eq!(issues[0].params.get("last"), Some(&ahead));
            assert_eq!(issues[0].params.get("type"), Some(&hot.code));
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
    let still_off = queries::get_permit_type(&db, hot.id).await.expect("q").expect("type");
    assert!(!still_off.requires_three_person_removal, "nothing written");
    // A plain edit (setting unchanged) is not a change of the setting: allowed.
    let mut renamed = type_input(&still_off, None, None);
    renamed.name = "Hot work (renamed)".into();
    queries::upsert_permit_type(&db, renamed, EDITOR)
        .await
        .expect("plain edit");

    // An unreadable recorded time: replaced by the new, readable record.
    set_held("not a time").await;
    let current = queries::get_permit_type(&db, hot.id).await.expect("q").expect("type");
    let on = queries::upsert_permit_type(&db, chain_input(&current, Some(true), Some(GOOD_REASON)), ADMIN_STEP_UP)
        .await
        .expect("unreadable held time");
    assert!(on.requires_three_person_removal);
    let at = scalar_text(
        &db,
        &format!("SELECT three_person_changed_at FROM permit_types WHERE id = {}", hot.id),
    )
    .await;
    assert!(crate::settings::site::instant(&at).is_some(), "{at}");
}

async fn scalar_text(db: &DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<String>(0)
        .expect("text")
}

/// Moves the recorded time of a type's last three-person change one minute back (tests that make
/// two changes of the same type within one second).
async fn backdate_three_person_change(db: &DatabaseConnection, type_id: i64) {
    let earlier = (chrono::Utc::now() - chrono::Duration::minutes(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE permit_types SET three_person_changed_at = ? WHERE id = ?",
        [earlier.into(), type_id.into()],
    ))
    .await
    .expect("backdate");
}

// ── Equipment / work-order change (CMMS Advisor decision A) ──────────

/// An edit of a permit's equipment (and, optionally, linked work order and expiry).
fn equipment_edit(
    wp: &WorkPermit,
    asset: Option<i64>,
    wo: Option<i64>,
    expires: Option<String>,
) -> WorkPermitUpdateInput {
    WorkPermitUpdateInput {
        id: wp.id,
        linked_work_order_id: wo,
        asset_id: asset,
        entity_id: None,
        expires_at: expires,
        expected_row_version: wp.row_version,
    }
}

async fn audit_count_of(db: &DatabaseConnection, action_code: &str, permit_id: i64) -> i64 {
    scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = '{action_code}' AND target_type = 'work_permit' \
             AND target_id = '{permit_id}' AND actor_id = {VERIFIER}"
        ),
    )
    .await
}

/// From `approved` on, a permit's equipment and work order are frozen ("approved for this
/// equipment"); its expiry can still be edited up to `issued`, as before.
#[tokio::test]
async fn an_approved_or_issued_permit_keeps_its_equipment_but_its_expiry_can_still_change() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-EQ1").await;
    let other = insert_equipment(&db, "EQ-PTW-EQ2").await;
    let mut wp = create_permit(&db, "hot_work", asset).await;
    for status in ["pending_review", "approved", "issued"] {
        wp = set_status(&db, &wp, status).await.expect("advance");
        if status == "pending_review" {
            continue;
        }
        let err = queries::update_work_permit(&db, equipment_edit(&wp, Some(other), None, None), VERIFIER)
            .await
            .unwrap_err();
        assert_eq!(refusal_code(err), "PERMIT_EQUIPMENT_LOCKED", "{status}");
        let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
        let err = queries::update_work_permit(&db, equipment_edit(&wp, None, Some(wo_id), None), VERIFIER)
            .await
            .unwrap_err();
        assert_eq!(refusal_code(err), "PERMIT_EQUIPMENT_LOCKED", "work order, {status}");
        // The expiry alone: allowed, nothing moves, nothing audited as an equipment change.
        let edited = queries::update_work_permit(
            &db,
            equipment_edit(
                &wp,
                Some(asset),
                None,
                Some((chrono::Utc::now() + chrono::Duration::hours(4)).to_rfc3339()),
            ),
            VERIFIER,
        )
        .await
        .unwrap_or_else(|e| panic!("expiry edit in {status}: {e:?}"));
        assert_eq!((edited.asset_id, edited.status.as_str()), (asset, status));
        wp = edited;
    }
    assert_eq!(audit_count_of(&db, "permit.equipment_changed", wp.id).await, 0);
}

/// A change while waiting for review sends the permit back to draft (audited): it must be
/// submitted again before anyone can approve it.
#[tokio::test]
async fn a_change_while_waiting_for_review_sends_the_permit_back_to_draft_for_a_fresh_review() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-EQ3").await;
    let other = insert_equipment(&db, "EQ-PTW-EQ4").await;
    let wp = create_permit(&db, "hot_work", asset).await;
    let wp = set_status(&db, &wp, "pending_review").await.expect("submit");
    let changed = queries::update_work_permit(&db, equipment_edit(&wp, Some(other), None, None), VERIFIER)
        .await
        .expect("change while waiting for review");
    assert_eq!((changed.asset_id, changed.status.as_str()), (other, "draft"));
    assert_eq!(audit_count_of(&db, "permit.equipment_changed", wp.id).await, 1);
    assert_eq!(audit_count_of(&db, "permit.review_reset", wp.id).await, 1);
    // The former review no longer counts: approving now is refused, it must be submitted again.
    let err = set_status(&db, &changed, "approved").await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_TRANSITION_NOT_ALLOWED");
    let again = set_status(&db, &changed, "pending_review").await.expect("submit again");
    set_status(&db, &again, "approved").await.expect("fresh review");

    // In draft the change stays a draft and records no review reset.
    let draft = create_permit(&db, "hot_work", asset).await;
    let moved = queries::update_work_permit(&db, equipment_edit(&draft, Some(other), None, None), VERIFIER)
        .await
        .expect("change in draft");
    assert_eq!(moved.status, "draft");
    assert_eq!(audit_count_of(&db, "permit.equipment_changed", draft.id).await, 1);
    assert_eq!(audit_count_of(&db, "permit.review_reset", draft.id).await, 0);
}

/// A lock fitted under the permit, or an isolation point planned on it, blocks the change (even
/// in draft): the points name the current equipment.
#[tokio::test]
async fn fitted_locks_and_planned_isolations_block_an_equipment_change() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-EQ5").await;
    let other = insert_equipment(&db, "EQ-PTW-EQ6").await;

    let planned = create_permit(&db, "loto", asset).await;
    add_isolation(&db, planned.id, "MCC-2 breaker").await;
    let err = queries::update_work_permit(&db, equipment_edit(&planned, Some(other), None, None), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_EQUIPMENT_ISOLATIONS_PLANNED");

    let fitted = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, fitted.id, "MCC-3 breaker").await;
    queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("fit the lock");
    let fitted = set_status(&db, &fitted, "pending_review").await.expect("submit");
    let err = queries::update_work_permit(&db, equipment_edit(&fitted, Some(other), None, None), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_EQUIPMENT_LOCK_FITTED");
    let kept = queries::get_work_permit(&db, fitted.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!((kept.asset_id, kept.status.as_str()), (asset, "pending_review"));
    assert_eq!(audit_count_of(&db, "permit.equipment_changed", fitted.id).await, 0);
}

/// The permit's equipment is its work order's, whichever of the two the edit changes.
#[tokio::test]
async fn a_permit_stays_on_its_work_orders_equipment() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-EQ7").await;
    let other = insert_equipment(&db, "EQ-PTW-EQ8").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let (other_wo, _) = assigned_wo_requiring_permit(&db, other).await;
    let linked = create_from_wo(&db, "hot_work", wo_id)
        .await
        .expect("permit on the work order");

    // The equipment alone: no longer the work order's.
    let err = queries::update_work_permit(&db, equipment_edit(&linked, Some(other), None, None), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_WO_EQUIPMENT_MISMATCH");
    // The work order alone: on other equipment.
    let err = queries::update_work_permit(&db, equipment_edit(&linked, None, Some(other_wo), None), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_WO_EQUIPMENT_MISMATCH");
    // Both together: aligned, applied.
    let moved = queries::update_work_permit(
        &db,
        equipment_edit(&linked, Some(other), Some(other_wo), None),
        VERIFIER,
    )
    .await
    .expect("equipment and work order together");
    assert_eq!((moved.asset_id, moved.linked_work_order_id), (other, Some(other_wo)));

    // A permit without a work order, linked to one on other equipment.
    let free = create_permit(&db, "hot_work", asset).await;
    let (third_wo, _) = assigned_wo_requiring_permit(&db, other).await;
    let err = queries::update_work_permit(&db, equipment_edit(&free, None, Some(third_wo), None), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_WO_EQUIPMENT_MISMATCH");
}

// ── Cancelling a planned isolation line (CMMS Advisor decision) ──────

fn cancel_input(iso: &PermitIsolation) -> crate::permit::domain::PermitIsolationCancelInput {
    crate::permit::domain::PermitIsolationCancelInput {
        isolation_id: iso.id,
        expected_row_version: iso.row_version,
    }
}

async fn isolation_now(db: &DatabaseConnection, permit_id: i64, id: i64) -> PermitIsolation {
    queries::list_permit_isolations(db, permit_id)
        .await
        .expect("list")
        .into_iter()
        .find(|i| i.id == id)
        .expect("the line is kept")
}

/// In draft, a planned line is cancelled, never deleted: who and when are on the line, its content
/// is kept, the cancellation is audited with that content, and the line is then frozen (no second
/// cancellation, no edit, no apply, no LOTO card).
#[tokio::test]
async fn a_planned_isolation_line_is_cancelled_kept_and_audited_in_draft() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-CX1").await;
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "MCC-9 breaker").await;

    let (permit, cancelled) = queries::cancel_permit_isolation(&db, cancel_input(&iso), VERIFIER)
        .await
        .expect("cancel a planned line in draft");
    assert_eq!(permit.status, "draft");
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(cancelled.cancelled_by_id, Some(VERIFIER));
    assert_eq!(cancelled.cancelled_by_name.as_deref(), Some("PTW Verifier"));
    assert_eq!(cancelled.isolation_point, "MCC-9 breaker");
    assert_eq!(
        (cancelled.energy_type_id, cancelled.isolation_method_id),
        (iso.energy_type_id, iso.isolation_method_id)
    );
    assert_eq!(cancelled.row_version, iso.row_version + 1);
    let kept = isolation_now(&db, wp.id, iso.id).await;
    assert!(kept.cancelled_at.is_some(), "kept, marked cancelled");

    assert_eq!(audit_count_of(&db, "permit.isolation_cancelled", wp.id).await, 1);
    assert_eq!(audit_count_of(&db, "permit.review_reset", wp.id).await, 0);
    let details = scalar_text(
        &db,
        &format!(
            "SELECT details_json FROM audit_events WHERE action_code = 'permit.isolation_cancelled' \
             AND target_id = '{}'",
            wp.id
        ),
    )
    .await;
    let details: serde_json::Value = serde_json::from_str(&details).expect("details");
    assert_eq!(details["isolation_point"], "MCC-9 breaker");
    assert_eq!(details["isolation_id"], iso.id);
    assert_eq!(details["energy_type_id"], serde_json::json!(iso.energy_type_id));
    assert_eq!(
        details["isolation_method_id"],
        serde_json::json!(iso.isolation_method_id)
    );
    assert!(details["cancelled_at"].is_string());

    let err = queries::cancel_permit_isolation(&db, cancel_input(&cancelled), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCELLED");
    let err = queries::apply_permit_isolation(&db, action(&cancelled), APPLIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCELLED");
    let err = queries::verify_permit_isolation(&db, action(&cancelled), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCELLED");
    let err = queries::upsert_permit_isolation(
        &db,
        PermitIsolationUpsertInput {
            id: Some(cancelled.id),
            permit_id: wp.id,
            isolation_point: "Renamed".into(),
            energy_type_id: energy(&db, "ELECTRICAL").await,
            isolation_method_id: method(&db, "BREAKER_LOCKOUT").await,
            lock_number: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCELLED");
    let err = queries::get_loto_card_view(&db, wp.id, cancelled.id).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCELLED");
    assert_eq!(
        isolation_now(&db, wp.id, iso.id).await.isolation_point,
        "MCC-9 breaker",
        "content unchanged"
    );
}

/// Waiting for review, a cancellation sends the permit back to draft (audited as a review reset
/// with its reason): it must be submitted again.
#[tokio::test]
async fn cancelling_a_line_while_waiting_for_review_sends_the_permit_back_to_draft() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-CX2").await;
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "MCC-10 breaker").await;
    let wp = set_status(&db, &wp, "pending_review").await.expect("submit");

    let (permit, _) = queries::cancel_permit_isolation(&db, cancel_input(&iso), VERIFIER)
        .await
        .expect("cancel while waiting for review");
    assert_eq!(permit.status, "draft");
    assert_eq!(permit.row_version, wp.row_version + 1);
    assert_eq!(audit_count_of(&db, "permit.isolation_cancelled", wp.id).await, 1);
    assert_eq!(audit_count_of(&db, "permit.review_reset", wp.id).await, 1);
    let reason = scalar_text(
        &db,
        &format!(
            "SELECT json_extract(details_json, '$.reason') FROM audit_events \
             WHERE action_code = 'permit.review_reset' AND target_id = '{}'",
            wp.id
        ),
    )
    .await;
    assert_eq!(reason, "isolation_cancelled");
    let err = set_status(&db, &permit, "approved").await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_TRANSITION_NOT_ALLOWED");
}

/// An applied line is never cancelled (the lock-removal path stays the only one), nor any line of
/// a permit approved or further on; a stale version is refused. Nothing is written.
#[tokio::test]
async fn an_applied_line_or_an_approved_permit_refuses_the_cancellation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-CX3").await;

    let draft = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, draft.id, "MCC-11 breaker").await;
    let applied = queries::apply_permit_isolation(&db, action(&iso), APPLIER)
        .await
        .expect("fit the lock");
    let err = queries::cancel_permit_isolation(&db, cancel_input(&applied), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCEL_APPLIED");
    let verified = queries::verify_permit_isolation(&db, action(&applied), VERIFIER)
        .await
        .expect("verify");
    let err = queries::cancel_permit_isolation(&db, cancel_input(&verified), APPLIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCEL_APPLIED");

    let mut wp = create_permit(&db, "loto", asset).await;
    let planned = add_isolation(&db, wp.id, "MCC-12 breaker").await;
    for status in ["pending_review", "approved", "issued"] {
        wp = set_status(&db, &wp, status).await.expect("advance");
        if status == "pending_review" {
            continue;
        }
        let err = queries::cancel_permit_isolation(&db, cancel_input(&planned), VERIFIER)
            .await
            .unwrap_err();
        assert_eq!(refusal_code(err), "PERMIT_ISOLATION_CANCEL_WRONG_STATUS", "{status}");
    }
    let stale = crate::permit::domain::PermitIsolationCancelInput {
        isolation_id: planned.id,
        expected_row_version: planned.row_version + 7,
    };
    let err = queries::cancel_permit_isolation(&db, stale, VERIFIER)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ValidationFailed(_)), "{err:?}");
    assert!(isolation_now(&db, wp.id, planned.id).await.cancelled_at.is_none());
    assert_eq!(audit_count_of(&db, "permit.isolation_cancelled", wp.id).await, 0);
    assert_eq!(audit_count_of(&db, "permit.isolation_cancelled", draft.id).await, 0);
}

/// A cancelled line no longer blocks a change of the permit's equipment.
#[tokio::test]
async fn a_cancelled_line_no_longer_blocks_the_equipment_change() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-CX4").await;
    let other = insert_equipment(&db, "EQ-PTW-CX5").await;
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "Old pump breaker").await;
    let err = queries::update_work_permit(&db, equipment_edit(&wp, Some(other), None, None), VERIFIER)
        .await
        .unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_EQUIPMENT_ISOLATIONS_PLANNED");
    queries::cancel_permit_isolation(&db, cancel_input(&iso), VERIFIER)
        .await
        .expect("cancel");
    let wp = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    let moved = queries::update_work_permit(&db, equipment_edit(&wp, Some(other), None, None), VERIFIER)
        .await
        .expect("equipment change once the planned line is cancelled");
    assert_eq!(moved.asset_id, other);
    // The cancelled line stays on the permit as history.
    assert_eq!(
        queries::list_permit_isolations(&db, wp.id).await.expect("list").len(),
        1
    );
}

/// A cancelled line is no isolation to fit: activation, close, the planning readiness and the
/// lockout count leave it out; a LOTO permit whose only line is cancelled has no isolation.
#[tokio::test]
async fn a_cancelled_line_is_not_counted_by_the_lockout_and_readiness_readers() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-CX6").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = create_from_wo(&db, "loto", wo_id)
        .await
        .expect("permit on the work order");
    let kept = add_isolation(&db, wp.id, "MCC-13 breaker").await;
    let dropped = add_isolation(&db, wp.id, "Valve V-13 (not needed)").await;
    let kept = apply_and_verify(&db, &kept).await;
    let loto_codes = |findings: Vec<crate::planning::readiness::Finding>| {
        findings
            .into_iter()
            .filter(|f| f.reason_code == "LOTO_NOT_VERIFIED")
            .count()
    };
    let before = crate::planning::readiness::permit_findings(&db, "work_order", wo_id, None)
        .await
        .expect("findings");
    assert_eq!(loto_codes(before), 1, "the planned line is not verified yet");
    queries::cancel_permit_isolation(&db, cancel_input(&dropped), VERIFIER)
        .await
        .expect("cancel");
    let after = crate::planning::readiness::permit_findings(&db, "work_order", wo_id, None)
        .await
        .expect("findings");
    assert_eq!(loto_codes(after), 0, "the cancelled line is no isolation to fit");

    let locks_before = queries::isolation_locks_in_place(&db).await.expect("locks");
    let wp = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    let wp = advance_to_issued(&db, wp).await;
    let wp = set_status(&db, &wp, "active")
        .await
        .expect("activation counts the verified line only");
    let kept = take_off(&db, &kept).await;
    queries::verify_permit_isolation_removal(&db, action(&kept), VERIFIER)
        .await
        .expect("remove the fitted lock");
    let closed = set_status(&db, &wp, "closed")
        .await
        .expect("close: the cancelled line has no lock");
    assert_eq!(closed.status, "closed");
    let locks_after = queries::isolation_locks_in_place(&db).await.expect("locks");
    assert_eq!(
        locks_after.1,
        locks_before.1 - 1,
        "only the fitted lock was counted, never the cancelled line"
    );

    // A LOTO permit whose only line is cancelled has no isolation: activation refused.
    let only = create_permit(&db, "loto", asset).await;
    let line = add_isolation(&db, only.id, "MCC-14 breaker").await;
    queries::cancel_permit_isolation(&db, cancel_input(&line), VERIFIER)
        .await
        .expect("cancel");
    let only = queries::get_work_permit(&db, only.id)
        .await
        .expect("read")
        .expect("permit");
    let only = advance_to_issued(&db, only).await;
    let err = set_status(&db, &only, "active").await.unwrap_err();
    assert_eq!(refusal_code(err), "LOTO_NO_ISOLATION");
}

/// Owner decision: a work order does not start or resume on a permit that expires before its
/// planned end (the work would outlive its protection). Without a planned end or an expected
/// duration nothing is judged.
#[tokio::test]
async fn wo_gate_refuses_a_permit_expiring_before_the_planned_end() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-95").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-95").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let expires = wp.expires_at.clone().expect("expiry");
    let set_planned_end = |end: Option<String>| {
        let db = &db;
        async move {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET planned_end = ? WHERE id = ?",
                [end.into(), wo_id.into()],
            ))
            .await
            .expect("planned end");
        }
    };

    // Planned to end a day after the permit expires: refused, with the times.
    set_planned_end(Some((chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339())).await;
    let err = start(&db, wo_id, rv).await.expect_err("expires before the planned end");
    match &err {
        AppError::OrgValidationFailed(issues) => {
            let issue = &issues[0];
            assert_eq!(issue.code, "PERMIT_EXPIRES_BEFORE_PLANNED_END");
            assert_eq!(issue.params.get("permit"), Some(&wp.code));
            assert_eq!(issue.params.get("expires_at"), Some(&expires));
            assert!(issue.params.contains_key("planned_end"));
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
    // A planned end typed without a zone (a form's local date and time) is read in this computer's
    // time zone, as planning reads it, and judged; the refusal gives it as a stored (UTC) time.
    let typed = (chrono::Local::now() + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M")
        .to_string();
    set_planned_end(Some(typed.clone())).await;
    let err = start(&db, wo_id, rv)
        .await
        .expect_err("typed local end after the expiry");
    match &err {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues[0].code, "PERMIT_EXPIRES_BEFORE_PLANNED_END");
            assert_eq!(issues[0].params.get("planned_end"), Some(&utc_of_local(&typed)));
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
    // Typed without a zone, before the expiry: not refused as unreadable (checked on the gate
    // itself, so the order stays ready for the cases below).
    let typed_soon = (chrono::Local::now() + chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M")
        .to_string();
    set_planned_end(Some(typed_soon)).await;
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id)
        .await
        .expect("typed local end before the expiry");
    // An unreadable planned end (text, or a bare day with no time) is refused with its own code
    // ("re-plan the dates"), never guessed.
    for bad in ["someday", "2026-10-20"] {
        set_planned_end(Some(bad.into())).await;
        let err = start(&db, wo_id, rv).await.expect_err("unreadable planned end");
        match &err {
            AppError::OrgValidationFailed(issues) => {
                assert_eq!(issues[0].code, "WO_PLANNED_END_UNREADABLE", "{bad}");
                assert_eq!(issues[0].params.get("value").map(String::as_str), Some(bad));
            }
            other => panic!("expected a coded refusal for {bad}, got {other:?}"),
        }
    }
    // No planned end and no expected duration: not judged.
    set_planned_end(None).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET expected_duration_hours = NULL WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("no duration");
    start(&db, wo_id, rv).await.expect("no planned end");

    // Resume: the same gate.
    let actor = admin_id(&db).await;
    let rv = scalar_i64(&db, &format!("SELECT row_version FROM work_orders WHERE id = {wo_id}")).await;
    let reason = scalar_i64(
        &db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS' AND rv.is_active = 1 AND rs.status = 'published' LIMIT 1",
    )
    .await;
    let paused = execution::pause_wo(
        &db,
        execution::WoPauseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            delay_reason_id: reason,
            comment: None,
        },
    )
    .await
    .expect("pause");
    set_planned_end(Some((chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339())).await;
    let resume = |rv: i64| {
        execution::resume_wo(
            &db,
            execution::WoResumeInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                estimated_remaining_hours: None,
            },
        )
    };
    assert_validation(resume(paused.row_version).await, "PERMIT_EXPIRES_BEFORE_PLANNED_END");
    // Planned to end before the expiry: resumes.
    set_planned_end(Some((chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339())).await;
    resume(paused.row_version).await.expect("covered by the permit");
}

/// A date and time typed without a zone (`YYYY-MM-DDTHH:MM`), as the UTC instant this computer's
/// time zone gives it, in the stored form.
fn utc_of_local(typed: &str) -> String {
    let local = chrono::NaiveDateTime::parse_from_str(typed, "%Y-%m-%dT%H:%M").expect("typed");
    chrono::TimeZone::from_local_datetime(&chrono::Local, &local)
        .earliest()
        .expect("local time")
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// CMMS Advisor decision (2026-10-19): without a planned end, the end of the work is the moment of
/// the start or resume plus the expected duration; the refusal names that estimated end. Without
/// either nothing is judged, but an expired permit is always refused.
#[tokio::test]
async fn wo_gate_judges_the_estimated_end_without_a_planned_end() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-96").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-96").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let expires = wp.expires_at.clone().expect("expiry");
    let set_plan = |end: Option<&'static str>, hours: Option<f64>| {
        let db = &db;
        async move {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET planned_end = ?, expected_duration_hours = ? WHERE id = ?",
                [end.into(), hours.into(), wo_id.into()],
            ))
            .await
            .expect("plan");
        }
    };

    // No planned end; 48 h of work from now outlives the permit (8 h): refused, naming the
    // estimated end and the duration it was computed with.
    set_plan(None, Some(48.0)).await;
    let before = chrono::Utc::now();
    let err = start(&db, wo_id, rv).await.expect_err("estimated end after the expiry");
    match &err {
        AppError::OrgValidationFailed(issues) => {
            let issue = &issues[0];
            assert_eq!(issue.code, "PERMIT_EXPIRES_BEFORE_ESTIMATED_END");
            assert_eq!(issue.params.get("permit"), Some(&wp.code));
            assert_eq!(issue.params.get("expires_at"), Some(&expires));
            assert_eq!(issue.params.get("hours").map(String::as_str), Some("48"));
            let end = crate::settings::site::instant(issue.params.get("end_at").expect("end_at")).expect("time");
            assert!(end >= before + chrono::Duration::hours(48) - chrono::Duration::seconds(1));
            assert!(end <= chrono::Utc::now() + chrono::Duration::hours(48));
            assert!(issue.message.contains("estimated end"), "{}", issue.message);
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
    // A zero duration is no estimate: not judged.
    set_plan(None, Some(0.0)).await;
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id)
        .await
        .expect("no estimate");
    // A planned end later than now + the duration wins: planned in two hours (before the 8 h
    // expiry), one hour of work: judged on the planned end, covered.
    let in_two_hours = (chrono::Utc::now() + chrono::Duration::hours(2))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET planned_end = ?, expected_duration_hours = 1.0 WHERE id = ?",
        [in_two_hours.into(), wo_id.into()],
    ))
    .await
    .expect("plan");
    crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id)
        .await
        .expect("judged on the planned end");
    // A planned end already past no longer hides the duration (review SF1): 48 h of work from now
    // outlives the permit.
    set_plan(Some("2026-04-10T16:00:00Z"), Some(48.0)).await;
    assert_validation(
        crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await,
        "PERMIT_EXPIRES_BEFORE_ESTIMATED_END",
    );

    // Resume: judged at the resume moment.
    set_plan(None, Some(1.0)).await;
    start(&db, wo_id, rv).await.expect("one hour of work is covered");
    let actor = admin_id(&db).await;
    let rv = scalar_i64(&db, &format!("SELECT row_version FROM work_orders WHERE id = {wo_id}")).await;
    let reason = scalar_i64(
        &db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS' AND rv.is_active = 1 AND rs.status = 'published' LIMIT 1",
    )
    .await;
    let pause = |rv: i64| {
        execution::pause_wo(
            &db,
            execution::WoPauseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                delay_reason_id: reason,
                comment: None,
            },
        )
    };
    let resume = |rv: i64| {
        execution::resume_wo(
            &db,
            execution::WoResumeInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                estimated_remaining_hours: None,
            },
        )
    };
    let paused = pause(rv).await.expect("pause");
    set_plan(None, Some(10.0)).await;
    assert_validation(resume(paused.row_version).await, "PERMIT_EXPIRES_BEFORE_ESTIMATED_END");
    // Neither a planned end nor a duration: not judged.
    set_plan(None, None).await;
    let resumed = resume(paused.row_version).await.expect("not judged");

    // An expired permit is always refused, with or without an end.
    let paused = pause(resumed.row_version).await.expect("pause again");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET expires_at = ? WHERE id = ?",
        [
            (chrono::Utc::now() - chrono::Duration::minutes(1))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
                .into(),
            wp.id.into(),
        ],
    ))
    .await
    .expect("expire");
    assert_validation(resume(paused.row_version).await, "PERMIT_EXPIRED");
}

/// An instant in the stored form (UTC, `Z`, whole seconds).
fn z(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Review SF1, CMMS Advisor rule: the end of the work is the later of the planned end and the
/// moment of the start, resume or reopening plus the expected duration (or the estimated remaining
/// time entered on a resume or reopening). The overnight case: planned yesterday 08:00-12:00 (4 h),
/// on hold overnight, resumed this morning on a permit expiring five minutes later: refused (the
/// old rule judged the past planned end and let it through). A remaining time entered on the
/// resume replaces the duration for this check only: refused at 0 or less, accepted when it fits,
/// recorded on the resume's history row (who, when, the value), the planned dates and the expected
/// duration unchanged. A planned end already past with no duration: refused with "re-plan the
/// dates or enter the duration / remaining time". The same on a reopening.
#[tokio::test]
async fn wo_gate_judges_late_work_on_the_moment_plus_the_duration_or_the_remaining_time() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-98").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-98").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let actor = admin_id(&db).await;
    let now = chrono::Utc::now();
    // Planned "yesterday 08:00-12:00": 25 h and 21 h ago.
    let planned_start = z(now - chrono::Duration::hours(25));
    let planned_end = z(now - chrono::Duration::hours(21));
    let plan = |hours: Option<f64>| {
        let db = &db;
        let (planned_start, planned_end) = (planned_start.clone(), planned_end.clone());
        async move {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET planned_start = ?, planned_end = ?, expected_duration_hours = ? WHERE id = ?",
                [planned_start.into(), planned_end.into(), hours.into(), wo_id.into()],
            ))
            .await
            .expect("plan");
        }
    };
    let code_of = |res: Result<crate::wo::domain::WorkOrder, AppError>| match res {
        Err(AppError::OrgValidationFailed(issues)) => issues[0].clone(),
        other => panic!("expected a coded refusal, got {other:?}"),
    };

    // A start after the planned end with no duration: the end of the work is unknown, refused with
    // its own code, which offers both ways out.
    plan(None).await;
    match crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await {
        Err(AppError::OrgValidationFailed(issues)) => {
            assert_eq!(issues[0].code, "WO_PLANNED_END_PASSED");
            assert_eq!(issues[0].params.get("planned_end"), Some(&planned_end));
            assert_eq!(issues[0].params.get("permit"), Some(&wp.code));
            assert!(issues[0].message.contains("re-plan"), "{}", issues[0].message);
            assert!(issues[0].message.contains("expected duration"), "{}", issues[0].message);
        }
        other => panic!("expected WO_PLANNED_END_PASSED, got {other:?}"),
    }
    // With 4 h of work: now + 4 h is covered by the 8 h permit.
    plan(Some(4.0)).await;
    start(&db, wo_id, rv).await.expect("4 h from now is covered");

    // On hold overnight; this morning the permit expires in five minutes.
    let reason = scalar_i64(
        &db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS' AND rv.is_active = 1 AND rs.status = 'published' LIMIT 1",
    )
    .await;
    let rv = scalar_i64(&db, &format!("SELECT row_version FROM work_orders WHERE id = {wo_id}")).await;
    let paused = execution::pause_wo(
        &db,
        execution::WoPauseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            delay_reason_id: reason,
            comment: None,
        },
    )
    .await
    .expect("pause");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET expires_at = ? WHERE id = ?",
        [
            z(chrono::Utc::now() + chrono::Duration::minutes(5)).into(),
            wp.id.into(),
        ],
    ))
    .await
    .expect("expires in five minutes");
    let resume = |rv: i64, remaining: Option<f64>| {
        execution::resume_wo(
            &db,
            execution::WoResumeInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                estimated_remaining_hours: remaining,
            },
        )
    };
    // The full 4 h from now outlives the permit: refused, naming the hours.
    let issue = code_of(resume(paused.row_version, None).await);
    assert_eq!(issue.code, "PERMIT_EXPIRES_BEFORE_ESTIMATED_END");
    assert_eq!(issue.params.get("hours").map(String::as_str), Some("4"));
    assert!(issue.message.contains("the expected duration"), "{}", issue.message);
    assert!(issue.message.contains("extend this permit"), "{}", issue.message);
    // An estimated remaining time of 0 or less (or not a number) is refused.
    for bad in [0.0, -1.0, f64::NAN] {
        assert_eq!(
            code_of(resume(paused.row_version, Some(bad)).await).code,
            "WO_REMAINING_TIME_INVALID",
            "{bad}"
        );
    }
    // Two hours remaining still outlive the permit: refused, naming the remaining time.
    let issue = code_of(resume(paused.row_version, Some(2.0)).await);
    assert_eq!(issue.code, "PERMIT_EXPIRES_BEFORE_ESTIMATED_END");
    assert_eq!(issue.params.get("hours").map(String::as_str), Some("2"));
    assert!(issue.message.contains("remaining time"), "{}", issue.message);
    // Three minutes remaining fit before the expiry: resumed.
    let resumed = resume(paused.row_version, Some(0.05)).await.expect("three minutes fit");
    assert_eq!(resumed.status_code.as_deref(), Some("in_progress"));
    // Recorded on the resume's history row (who, when, the value); the planned dates and the
    // expected duration are unchanged.
    let recorded = |action: &'static str| {
        let db = &db;
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT actor_id, acted_at, estimated_remaining_hours FROM wo_state_transition_log \
                 WHERE wo_id = ? AND action = ? AND to_status = 'in_progress' ORDER BY id DESC LIMIT 1",
                [wo_id.into(), action.into()],
            ))
            .await
            .expect("query")
            .map(|r| {
                (
                    r.try_get::<i64>("", "actor_id").expect("actor"),
                    r.try_get::<String>("", "acted_at").expect("acted_at"),
                    r.try_get::<Option<f64>>("", "estimated_remaining_hours")
                        .expect("hours"),
                )
            })
            .expect("transition row")
        }
    };
    let (who, when, hours) = recorded("resume").await;
    assert_eq!((who, hours), (actor, Some(0.05)));
    assert!(crate::settings::site::instant(&when).is_some(), "{when}");
    assert_eq!(
        scalar_text(
            &db,
            &format!("SELECT planned_end || '|' || expected_duration_hours FROM work_orders WHERE id = {wo_id}")
        )
        .await,
        format!("{planned_end}|4.0")
    );
    // A start (or a resume without a remaining time) records none.
    assert_eq!(recorded("start").await.2, None);

    // Planned end past and no duration on a resume: refused with the code that offers the
    // remaining time; a remaining time that fits lets it resume.
    let paused = execution::pause_wo(
        &db,
        execution::WoPauseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: resumed.row_version,
            delay_reason_id: reason,
            comment: None,
        },
    )
    .await
    .expect("pause again");
    plan(None).await;
    let issue = code_of(resume(paused.row_version, None).await);
    assert_eq!(issue.code, "WO_PLANNED_END_PASSED");
    assert!(issue.message.contains("remaining time"), "{}", issue.message);
    let resumed = resume(paused.row_version, Some(0.05))
        .await
        .expect("remaining time fits");

    // Reopening to in progress: the same rule, with its own remaining time.
    let completed = scalar_i64(&db, "SELECT id FROM work_order_statuses WHERE code = 'completed'").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET status_id = ? WHERE id = ?",
        [completed.into(), wo_id.into()],
    ))
    .await
    .expect("completed");
    let reopen = |rv: i64, remaining: Option<f64>| {
        crate::wo::closeout::reopen_wo(
            &db,
            crate::wo::closeout::WoReopenInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                reason: "Leak came back".into(),
                target_status: None,
                estimated_remaining_hours: remaining,
            },
        )
    };
    let rv = resumed.row_version;
    assert_eq!(code_of(reopen(rv, None).await).code, "WO_PLANNED_END_PASSED");
    plan(Some(4.0)).await;
    assert_eq!(
        code_of(reopen(rv, None).await).code,
        "PERMIT_EXPIRES_BEFORE_ESTIMATED_END"
    );
    assert_eq!(code_of(reopen(rv, Some(-2.0)).await).code, "WO_REMAINING_TIME_INVALID");
    let reopened = reopen(rv, Some(0.05)).await.expect("remaining time fits");
    assert_eq!(reopened.status_code.as_deref(), Some("in_progress"));
    let (who, _, hours) = recorded("reopen").await;
    assert_eq!((who, hours), (actor, Some(0.05)));
    assert_eq!(
        scalar_text(
            &db,
            &format!("SELECT CAST(expected_duration_hours AS TEXT) FROM work_orders WHERE id = {wo_id}")
        )
        .await,
        "4.0"
    );
}

/// The planned dates typed in the work order form (a `datetime-local` value, no zone) are stored as
/// UTC instants read in this computer's time zone (never read as UTC); text that is not a date and
/// time is refused with a coded "re-plan the dates", at create and at draft edit.
#[tokio::test]
async fn planned_dates_typed_without_a_zone_are_stored_as_instants() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-97").await;
    let actor = admin_id(&db).await;
    let input = |start: Option<&str>, end: Option<&str>| WoCreateInput {
        type_code: "corrective".into(),
        equipment_id: Some(asset),
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
        title: "Typed dates".into(),
        description: None,
        notes: None,
        planned_start: start.map(str::to_string),
        planned_end: end.map(str::to_string),
        shift: None,
        expected_duration_hours: None,
        creator_id: actor,
        requires_permit: Some(false),
    };
    let planned = |column: &'static str, id: i64| {
        let db = &db;
        async move { scalar_text(db, &format!("SELECT {column} FROM work_orders WHERE id = {id}")).await }
    };
    let wo = crate::wo::queries::create_work_order(&db, input(Some("2026-10-20T08:00"), Some("2026-10-20T14:30")))
        .await
        .expect("create");
    assert_eq!(planned("planned_start", wo.id).await, utc_of_local("2026-10-20T08:00"));
    assert_eq!(planned("planned_end", wo.id).await, utc_of_local("2026-10-20T14:30"));
    // An instant with an offset is stored in UTC.
    let other = crate::wo::queries::create_work_order(&db, input(None, Some("2026-10-20T14:30:00+02:00")))
        .await
        .expect("create");
    assert_eq!(planned("planned_end", other.id).await, "2026-10-20T12:30:00Z");
    for (start, end, code) in [
        (Some("tomorrow"), None, "WO_PLANNED_START_UNREADABLE"),
        (None, Some("2026-10-20"), "WO_PLANNED_END_UNREADABLE"),
    ] {
        match crate::wo::queries::create_work_order(&db, input(start, end)).await {
            Err(AppError::OrgValidationFailed(issues)) => assert_eq!(issues[0].code, code),
            other => panic!("expected {code}, got {other:?}"),
        }
    }

    // Draft edit: the same; a blank date clears it.
    let draft = |id: i64, rv: i64, end: &str| crate::wo::domain::WoDraftUpdateInput {
        id,
        expected_row_version: rv,
        title: None,
        type_code: None,
        equipment_id: None,
        location_id: None,
        description: None,
        planned_start: None,
        planned_end: Some(end.to_string()),
        shift: None,
        expected_duration_hours: None,
        notes: None,
        urgency_code: None,
        subtype_code: None,
        requires_permit: None,
    };
    let edited = crate::wo::queries::update_wo_draft_fields(&db, draft(wo.id, wo.row_version, "2026-10-21T09:15"))
        .await
        .expect("edit");
    assert_eq!(planned("planned_end", wo.id).await, utc_of_local("2026-10-21T09:15"));
    match crate::wo::queries::update_wo_draft_fields(&db, draft(wo.id, edited.row_version, "soon")).await {
        Err(AppError::OrgValidationFailed(issues)) => assert_eq!(issues[0].code, "WO_PLANNED_END_UNREADABLE"),
        other => panic!("expected WO_PLANNED_END_UNREADABLE, got {other:?}"),
    }
    crate::wo::queries::update_wo_draft_fields(&db, draft(wo.id, edited.row_version, "  "))
        .await
        .expect("clear");
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM work_orders WHERE id = {} AND planned_end IS NULL",
                wo.id
            )
        )
        .await,
        1
    );
}

/// A permit type's code is fixed once the type exists (plant rules, permits and work orders name the
/// type by it): a save that changes it is refused; the same code with spaces around it is kept as
/// stored.
#[tokio::test]
async fn a_permit_types_code_cannot_be_changed() {
    let db = setup().await;
    let hot = queries::list_permit_types(&db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == "hot_work")
        .expect("hot_work");
    let mut renamed = type_input(&hot, None, None);
    renamed.code = "hot_work_2".into();
    let err = queries::upsert_permit_type(&db, renamed, EDITOR)
        .await
        .expect_err("code change");
    match err {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues[0].code, "PERMIT_TYPE_CODE_LOCKED");
            assert_eq!(issues[0].params.get("type").map(String::as_str), Some("hot_work"));
            assert_eq!(issues[0].params.get("code").map(String::as_str), Some("hot_work_2"));
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
    let mut padded = type_input(&hot, None, None);
    padded.code = "  hot_work ".into();
    padded.name = "Hot work (renamed)".into();
    let saved = queries::upsert_permit_type(&db, padded, EDITOR)
        .await
        .expect("same code");
    assert_eq!(saved.code, "hot_work");
    assert_eq!(saved.name, "Hot work (renamed)");
}

/// The hand-over receiver search ignores case and accents and reads "%" and "_" as plain
/// characters; the total counts every match, whatever the page. The accounts hold the permit
/// close right (here plant-wide), as every receiver must at the permit's site.
#[tokio::test]
async fn the_receiver_search_ignores_case_and_accents_and_has_no_wildcards() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-PTW-RCV-SEARCH").await;
    let wp = create_permit(&db, "hot_work", asset).await;
    for (id, login, name) in [
        (501, "elodie.m", "Élodie Martin"),
        (502, "elodie.x", "elodie_x"),
        (503, "zoe", "Zoé 100%"),
    ] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active, \
             is_admin, force_password_change, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'local', 1, 0, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [
                id.into(),
                format!("user-sync-{login}").into(),
                login.into(),
                name.into(),
            ],
        ))
        .await
        .expect("account");
        grant_tenant_right(&db, id, crate::rbac::permissions::PTW_CLOSE).await;
    }
    let names = |search: &'static str, limit: Option<u64>, offset: Option<u64>| {
        let db = &db;
        async move {
            let page = queries::list_handover_receivers(db, APPLIER, wp.id, Some(search.into()), limit, offset)
                .await
                .expect("receivers");
            (
                page.items.into_iter().map(|r| r.display_name).collect::<Vec<_>>(),
                page.total,
            )
        }
    };
    assert_eq!(
        names("ELODIE", None, None).await,
        (vec!["Élodie Martin".to_string(), "elodie_x".to_string()], 2)
    );
    assert_eq!(
        names("élodie m", None, None).await,
        (vec!["Élodie Martin".to_string()], 1)
    );
    assert_eq!(names("_", None, None).await, (vec!["elodie_x".to_string()], 1));
    assert_eq!(names("%", None, None).await, (vec!["Zoé 100%".to_string()], 1));
    assert_eq!(names("ZOE", None, None).await, (vec!["Zoé 100%".to_string()], 1));
    // One page of one, the second match: the total still counts both.
    assert_eq!(
        names("elodie", Some(1), Some(1)).await,
        (vec!["elodie_x".to_string()], 2)
    );
}

/// The three-person change reason refusal names the minimum the server applies, so the screen can
/// show it whatever its own copy of the rule.
#[tokio::test]
async fn the_three_person_reason_refusal_names_the_minimum() {
    let db = setup().await;
    let hot = queries::list_permit_types(&db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == "hot_work")
        .expect("hot_work");
    let err = queries::upsert_permit_type(&db, chain_input(&hot, Some(true), Some("too short")), ADMIN_STEP_UP)
        .await
        .expect_err("short");
    match err {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues[0].code, "PERMIT_TYPE_THREE_PERSON_REASON");
            assert_eq!(
                issues[0].params.get("min"),
                Some(&crate::settings::catalog::MIN_REASON_CHARS.to_string())
            );
            assert_eq!(issues[0].params.get("type").map(String::as_str), Some("hot_work"));
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Four-eyes on permit approval (owner decision 2026-10-17, migration 244)
// ═══════════════════════════════════════════════════════════════════════════════

/// Makes the test site (org node 1, created with the first equipment) a planning site: its type
/// can own work. Without a per-site rule it keeps four-eyes.
pub(super) async fn make_test_site_a_planning_site(db: &DatabaseConnection) {
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE org_node_types SET can_own_work = 1 WHERE id = 1".to_string(),
    ))
    .await
    .expect("planning site");
}

/// Opts the test site (org node 1, created with the first equipment) out of four-eyes: a planning
/// site whose per-site rule (the one reliability decisions use) allows self-approval.
pub(super) async fn opt_site_out_of_four_eyes(db: &DatabaseConnection) {
    make_test_site_a_planning_site(db).await;
    for sql in [format!(
        "INSERT INTO site_settings (entity_sync_id, site_node_id, setting_key, value_json) \
             VALUES ('site-setting-ptw-test', 1, '{}', 'true')",
        crate::reliability::study::decisions::SELF_APPROVAL_SETTING
    )] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .expect("opt out");
    }
}

async fn audit_details(db: &DatabaseConnection, action: &str, permit_id: i64) -> Vec<serde_json::Value> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT details_json FROM audit_events WHERE action_code = '{action}' AND target_type = 'work_permit' \
             AND target_id = '{permit_id}' ORDER BY id"
        ),
    ))
    .await
    .expect("audit")
    .iter()
    .map(|r| serde_json::from_str(&r.try_get_by_index::<String>(0).expect("details")).expect("json"))
    .collect()
}

/// The approver is neither the person who wrote the permit nor the person who submitted it (each
/// refused on its own); a second person approves, recorded with the submitter, and both steps are
/// audited.
#[tokio::test]
async fn four_eyes_the_author_and_the_submitter_never_approve_a_permit() {
    let db = setup().await;
    let submitter = second_approver(&db).await;
    let asset = insert_equipment(&db, "EQ-4EYES-1").await;
    // A real planning site that keeps four-eyes (no per-site rule).
    make_test_site_a_planning_site(&db).await;
    let wp = create_permit(&db, "hot_work", asset).await;
    assert_eq!(wp.created_by_id, Some(APPLIER), "the author is the session user");
    let wp = set_status_as(&db, &wp, "pending_review", submitter)
        .await
        .expect("submit");
    assert_eq!(wp.submitted_by_id, Some(submitter));
    assert!(wp.submitted_at.is_some());
    assert_eq!((wp.approved_by_id, wp.approved_at.as_deref()), (None, None));

    let err = set_status_as(&db, &wp, "approved", APPLIER).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_APPROVER_IS_CREATOR");
    let err = set_status_as(&db, &wp, "approved", submitter).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_APPROVER_IS_SUBMITTER");
    let held = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(
        (held.status.as_str(), held.row_version),
        ("pending_review", wp.row_version)
    );

    let approved = set_status_as(&db, &wp, "approved", VERIFIER)
        .await
        .expect("a second person");
    assert_eq!(approved.approved_by_id, Some(VERIFIER));
    assert_eq!(approved.approved_by_name.as_deref(), Some("PTW Verifier"));
    assert!(approved.approved_at.is_some());
    assert!(!approved.self_approved);
    assert_eq!(approved.submitted_by_id, Some(submitter), "the submission is kept");
    assert_eq!(approved.created_by_id, Some(APPLIER));

    let submitted = audit_details(&db, "permit.submitted", wp.id).await;
    assert_eq!(submitted.len(), 1);
    let approvals = audit_details(&db, "permit.approved", wp.id).await;
    assert_eq!(approvals.len(), 1, "refusals write nothing");
    assert_eq!(approvals[0]["self_approved"], serde_json::json!(false));
    assert_eq!(approvals[0]["submitted_by_id"], serde_json::json!(submitter));

    // Further steps keep the record.
    let issued = set_status(&db, &approved, "issued").await.expect("issue");
    assert_eq!(
        (issued.approved_by_id, issued.submitted_by_id, issued.created_by_id),
        (Some(VERIFIER), Some(submitter), Some(APPLIER))
    );
}

/// On a site that opted out of four-eyes (the per-site rule), the author who submitted it may
/// approve: allowed, flagged "auto-approbation" on the permit and in the audit. The same approval
/// is refused once the site keeps four-eyes again.
#[tokio::test]
async fn four_eyes_an_opted_out_site_allows_a_flagged_self_approval() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-4EYES-2").await;
    opt_site_out_of_four_eyes(&db).await;
    let wp = create_permit(&db, "hot_work", asset).await;
    let wp = set_status_as(&db, &wp, "pending_review", APPLIER)
        .await
        .expect("submit");
    let approved = set_status_as(&db, &wp, "approved", APPLIER)
        .await
        .expect("self-approval allowed");
    assert!(approved.self_approved, "flagged");
    assert_eq!(approved.approved_by_id, Some(APPLIER));
    let approvals = audit_details(&db, "permit.approved", wp.id).await;
    assert_eq!(approvals[0]["self_approved"], serde_json::json!(true));
    assert_eq!(approvals[0]["site_rule"], serde_json::json!("four_eyes_opted_out"));

    // A second person on the same site: a four-eyes approval, not flagged.
    let other = create_permit(&db, "hot_work", asset).await;
    let other = set_status_as(&db, &other, "pending_review", APPLIER)
        .await
        .expect("submit");
    let other = set_status_as(&db, &other, "approved", VERIFIER).await.expect("approve");
    assert!(!other.self_approved);

    // The site keeps four-eyes again: refused.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE site_settings SET value_json = 'false'".to_string(),
    ))
    .await
    .expect("opt in");
    let third = create_permit(&db, "hot_work", asset).await;
    let third = set_status_as(&db, &third, "pending_review", APPLIER)
        .await
        .expect("submit");
    let err = set_status_as(&db, &third, "approved", APPLIER).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_APPROVER_IS_SUBMITTER");
}

/// A permit written before the author was recorded cannot show four-eyes: refused on a four-eyes
/// site (nobody is invented); on an opted-out site it is allowed and flagged.
#[tokio::test]
async fn four_eyes_an_unrecorded_author_is_never_taken_as_a_second_person() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-4EYES-3").await;
    make_test_site_a_planning_site(&db).await;
    let wp = create_permit(&db, "hot_work", asset).await;
    let wp = set_status_as(&db, &wp, "pending_review", APPLIER)
        .await
        .expect("submit");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        format!("UPDATE work_permits SET created_by_id = NULL WHERE id = {}", wp.id),
    ))
    .await
    .expect("as before the version");
    let err = set_status_as(&db, &wp, "approved", VERIFIER).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_APPROVAL_AUTHOR_UNKNOWN");

    opt_site_out_of_four_eyes(&db).await;
    let approved = set_status_as(&db, &wp, "approved", VERIFIER).await.expect("opted out");
    assert!(approved.self_approved, "no verified second person: flagged");
}

/// A permit sent back to draft while waiting for review (its equipment changed, or a planned
/// isolation line cancelled) loses its submission: it is submitted again, and the reset is audited
/// with the submission it cleared.
#[tokio::test]
async fn four_eyes_a_permit_back_to_draft_is_submitted_again() {
    let db = setup().await;
    let submitter = second_approver(&db).await;
    let asset = insert_equipment(&db, "EQ-4EYES-4").await;
    make_test_site_a_planning_site(&db).await;
    let other = insert_equipment(&db, "EQ-4EYES-5").await;

    // Equipment changed in review.
    let wp = create_permit(&db, "hot_work", asset).await;
    let wp = set_status_as(&db, &wp, "pending_review", submitter)
        .await
        .expect("submit");
    let back = queries::update_work_permit(
        &db,
        WorkPermitUpdateInput {
            id: wp.id,
            linked_work_order_id: None,
            asset_id: Some(other),
            entity_id: None,
            expires_at: None,
            expected_row_version: wp.row_version,
        },
        APPLIER,
    )
    .await
    .expect("equipment changed");
    assert_eq!(back.status, "draft");
    assert_eq!((back.submitted_by_id, back.submitted_at.as_deref()), (None, None));
    let resets = audit_details(&db, "permit.review_reset", wp.id).await;
    assert_eq!(resets[0]["cleared_submitted_by_id"], serde_json::json!(submitter));
    // Approving needs a fresh submission; the new submitter is the one judged.
    let err = set_status_as(&db, &back, "approved", VERIFIER).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_TRANSITION_NOT_ALLOWED");
    let again = set_status_as(&db, &back, "pending_review", VERIFIER)
        .await
        .expect("submit again");
    assert_eq!(again.submitted_by_id, Some(VERIFIER));
    let err = set_status_as(&db, &again, "approved", VERIFIER).await.unwrap_err();
    assert_eq!(refusal_code(err), "PERMIT_APPROVER_IS_SUBMITTER");
    set_status_as(&db, &again, "approved", submitter)
        .await
        .expect("the former submitter is a second person now");

    // A planned isolation line cancelled in review.
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "Q1 breaker").await;
    let submitted = set_status_as(&db, &wp, "pending_review", submitter)
        .await
        .expect("submit");
    assert_eq!(submitted.submitted_by_id, Some(submitter));
    let (back, _) = queries::cancel_permit_isolation(
        &db,
        crate::permit::domain::PermitIsolationCancelInput {
            isolation_id: iso.id,
            expected_row_version: iso.row_version,
        },
        APPLIER,
    )
    .await
    .expect("cancel line");
    assert_eq!(back.status, "draft");
    assert_eq!((back.submitted_by_id, back.submitted_at.as_deref()), (None, None));
}
