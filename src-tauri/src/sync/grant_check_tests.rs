//! Received grants re-checked against their author (`sync::grant_check`), through the real inbound
//! path (`apply_inbound_payload` → `row_codec::apply_row` → `apply_hooks::before_upsert_checked`),
//! with payloads read from the database as the codec sends them.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::sync::grant_check::GRANT_BY_UNAUTHORISED_ADMIN;
use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};

type Payload = Map<String, Value>;

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<i64> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .map(|r| r.try_get_by_index::<i64>(0).expect("integer"))
}

fn stamp(offset: chrono::Duration) -> String {
    (chrono::Utc::now() + offset).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn now() -> String {
    stamp(chrono::Duration::zero())
}

async fn user(db: &DatabaseConnection, username: &str) -> i64 {
    let at = now();
    exec(
        db,
        "INSERT INTO user_accounts \
         (entity_sync_id, username, display_name, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES (?, ?, ?, 'local', 'x', 1, 0, 0, ?, 0, ?, ?, 1)",
        vec![
            format!("{username}-sync").into(),
            username.into(),
            username.into(),
            at.clone().into(),
            at.clone().into(),
            at.into(),
        ],
    )
    .await;
    scalar(
        db,
        "SELECT id FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await
    .expect("user")
}

async fn role_id(db: &DatabaseConnection, name: &str) -> i64 {
    scalar(db, "SELECT id FROM roles WHERE name = ?", vec![name.into()])
        .await
        .unwrap_or_else(|| panic!("role {name}"))
}

async fn role_sync(db: &DatabaseConnection, name: &str) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT entity_sync_id FROM roles WHERE name = ?",
        [name.into()],
    ))
    .await
    .unwrap()
    .expect("role")
    .try_get::<String>("", "entity_sync_id")
    .expect("role sync id")
}

/// A custom role holding exactly `permissions`.
async fn custom_role(db: &DatabaseConnection, name: &str, permissions: &[&str]) -> i64 {
    let at = now();
    exec(
        db,
        "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
         VALUES (?, ?, 'custom', 'active', 0, ?, ?, 1)",
        vec![
            format!("{name}-sync").into(),
            name.into(),
            at.clone().into(),
            at.clone().into(),
        ],
    )
    .await;
    let role = role_id(db, name).await;
    for p in permissions {
        exec(
            db,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) SELECT ?, id, ? FROM permissions WHERE name = ?",
            vec![role.into(), at.clone().into(), (*p).into()],
        )
        .await;
    }
    role
}

/// A plant-wide assignment of `role` to `user` made by `by` at `created_at`; returns its sync id.
async fn assign(
    db: &DatabaseConnection,
    sync_id: &str,
    user: i64,
    role: i64,
    by: Option<i64>,
    created_at: &str,
) -> String {
    exec(
        db,
        "INSERT INTO user_scope_assignments \
         (entity_sync_id, user_id, role_id, scope_type, scope_reference, assigned_by_id, created_at, updated_at, \
          row_version) \
         VALUES (?, ?, ?, 'tenant', NULL, ?, ?, ?, 1)",
        vec![
            sync_id.into(),
            user.into(),
            role.into(),
            by.map_or(sea_orm::Value::BigInt(None), Into::into),
            created_at.into(),
            created_at.into(),
        ],
    )
    .await;
    sync_id.to_string()
}

async fn payload(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    crate::sync::test_support::current_payload(db, entity_type, sync_id, &Map::default())
        .await
        .expect("payload")
        .expect("row")
}

async fn receive(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
    operation: &str,
    row: Payload,
) -> ApplyOutcome {
    apply_inbound_payload(db, entity_type, sync_id, operation, 2, &Value::Object(row).to_string())
        .await
        .expect("apply")
}

/// The wire form of an assignment made on another PC: written here, read as the codec sends it,
/// removed again so that it arrives as new.
async fn remote_assignment(db: &DatabaseConnection, sync_id: &str, user: i64, role: i64, by: Option<i64>) -> Payload {
    assign(db, sync_id, user, role, by, &now()).await;
    let row = payload(db, "user_scope_assignments", sync_id).await;
    exec(
        db,
        "DELETE FROM user_scope_assignments WHERE entity_sync_id = ?",
        vec![sync_id.into()],
    )
    .await;
    row
}

async fn assignment_held(db: &DatabaseConnection, sync_id: &str) -> bool {
    scalar(
        db,
        "SELECT 1 FROM user_scope_assignments WHERE entity_sync_id = ? AND deleted_at IS NULL",
        vec![sync_id.into()],
    )
    .await
    .is_some()
}

fn is_grant_conflict(outcome: &ApplyOutcome) -> bool {
    matches!(outcome, ApplyOutcome::Conflict { code, .. } if code == GRANT_BY_UNAUTHORISED_ADMIN)
}

/// A plant administrator ("chef", Administrator role), a user manager ("manager", a custom role
/// holding `adm.users` and `adm.roles`) and a plain account ("worker").
struct Plant {
    db: DatabaseConnection,
    chef: i64,
    manager: i64,
    manager_assignment: String,
    worker: i64,
    operator: i64,
}

async fn plant() -> Plant {
    let db = crate::db::test_db::seeded().await;
    crate::sync::capture::install_capture_in(&db)
        .await
        .expect("capture + seeded identities");
    let chef = user(&db, "chef").await;
    let admin_role = role_id(&db, "Administrator").await;
    assign(&db, "chef-admin", chef, admin_role, None, &now()).await;
    let manager = user(&db, "manager").await;
    let manager_role = custom_role(
        &db,
        "user-managers",
        &[crate::rbac::permissions::ADM_USERS, crate::rbac::permissions::ADM_ROLES],
    )
    .await;
    let manager_assignment = assign(&db, "manager-role", manager, manager_role, Some(chef), &now()).await;
    let worker = user(&db, "worker").await;
    let operator = custom_role(&db, "field-operators", &["ot.view"]).await;
    Plant {
        db,
        chef,
        manager,
        manager_assignment,
        worker,
        operator,
    }
}

/// The manager's right withdrawn here (as received earlier from another PC).
async fn demote_manager(p: &Plant) {
    exec(
        &p.db,
        "UPDATE user_scope_assignments SET deleted_at = ? WHERE entity_sync_id = ?",
        vec![now().into(), p.manager_assignment.clone().into()],
    )
    .await;
}

#[tokio::test]
async fn a_grant_by_a_current_user_manager_is_applied() {
    let p = plant().await;
    let row = remote_assignment(&p.db, "grant-ok", p.worker, p.operator, Some(p.manager)).await;
    let outcome = receive(&p.db, "user_scope_assignments", "grant-ok", "upsert", row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(assignment_held(&p.db, "grant-ok").await);
}

#[tokio::test]
async fn a_grant_by_a_demoted_user_manager_is_a_conflict() {
    let p = plant().await;
    let row = remote_assignment(&p.db, "grant-late", p.worker, p.operator, Some(p.manager)).await;
    demote_manager(&p).await;
    let outcome = receive(&p.db, "user_scope_assignments", "grant-late", "upsert", row.clone()).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");
    assert!(!assignment_held(&p.db, "grant-late").await, "not applied");

    // "Take the received version" goes through the same check: still refused.
    let again = receive(&p.db, "user_scope_assignments", "grant-late", "upsert", row).await;
    assert!(is_grant_conflict(&again), "{again:?}");
}

#[tokio::test]
async fn a_grant_whose_author_is_not_received_yet_waits_for_that_account() {
    let p = plant().await;
    let mut row = remote_assignment(&p.db, "grant-wait", p.worker, p.operator, Some(p.manager)).await;
    row.insert("assigned_by_sync_id".into(), "someone-not-here-sync".into());
    let outcome = receive(&p.db, "user_scope_assignments", "grant-wait", "upsert", row).await;
    match outcome {
        ApplyOutcome::Deferred { code, waits_for, .. } => {
            assert_eq!(code, crate::sync::materialize::DEFER_MISSING_PARENT);
            let w = waits_for.expect("waits for the author");
            assert_eq!(
                (w.entity_type.as_str(), w.sync_id.as_str()),
                ("user_accounts", "someone-not-here-sync")
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(!assignment_held(&p.db, "grant-wait").await);
}

#[tokio::test]
async fn an_administrator_role_needs_a_plant_administrator() {
    let p = plant().await;
    let admin_role = role_id(&p.db, "Administrator").await;
    let row = remote_assignment(&p.db, "admin-by-manager", p.worker, admin_role, Some(p.manager)).await;
    let outcome = receive(&p.db, "user_scope_assignments", "admin-by-manager", "upsert", row).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");
    assert!(!assignment_held(&p.db, "admin-by-manager").await);

    let row = remote_assignment(&p.db, "admin-by-chef", p.worker, admin_role, Some(p.chef)).await;
    let outcome = receive(&p.db, "user_scope_assignments", "admin-by-chef", "upsert", row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn a_role_given_to_oneself_or_without_an_author_is_a_conflict() {
    let p = plant().await;
    let row = remote_assignment(&p.db, "self-grant", p.manager, p.operator, Some(p.manager)).await;
    let outcome = receive(&p.db, "user_scope_assignments", "self-grant", "upsert", row).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");

    let row = remote_assignment(&p.db, "no-author", p.worker, p.operator, None).await;
    let outcome = receive(&p.db, "user_scope_assignments", "no-author", "upsert", row).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");
}

/// Bootstrap rows without an author: the first administrator (bootstrap flag) and the
/// Administrator role given by a vendor recovery code (its audit record here).
#[tokio::test]
async fn bootstrap_and_recovery_assignments_without_an_author_are_applied() {
    let p = plant().await;
    let admin_role = role_id(&p.db, "Administrator").await;
    let first = user(&p.db, "first-admin").await;
    exec(
        &p.db,
        "UPDATE user_accounts SET is_admin = 1 WHERE id = ?",
        vec![first.into()],
    )
    .await;
    let row = remote_assignment(&p.db, "bootstrap-admin", first, admin_role, None).await;
    let outcome = receive(&p.db, "user_scope_assignments", "bootstrap-admin", "upsert", row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    let recovered = user(&p.db, "recovered-admin").await;
    let row = remote_assignment(&p.db, "recovery-admin", recovered, admin_role, None).await;
    let outcome = receive(&p.db, "user_scope_assignments", "recovery-admin", "upsert", row.clone()).await;
    assert!(is_grant_conflict(&outcome), "no recovery record: {outcome:?}");
    crate::audit::writer::write_audit_event_in(
        &p.db,
        crate::audit::writer::AuditEventInput {
            action_code: crate::auth::recovery_code::AUDIT_USED.to_string(),
            target_type: Some("user_account".to_string()),
            target_id: Some(recovered.to_string()),
            actor_id: None,
            auth_context: "signed_out".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "security".to_string(),
            details_json: None,
        },
    )
    .await
    .expect("audit record");
    let outcome = receive(&p.db, "user_scope_assignments", "recovery-admin", "upsert", row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn a_revocation_is_always_applied() {
    let p = plant().await;
    assign(&p.db, "to-revoke", p.worker, p.operator, Some(p.manager), &now()).await;
    let mut row = payload(&p.db, "user_scope_assignments", "to-revoke").await;
    demote_manager(&p).await;
    row.insert("deleted_at".into(), now().into());
    let outcome = receive(&p.db, "user_scope_assignments", "to-revoke", "upsert", row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(!assignment_held(&p.db, "to-revoke").await);

    // Restored by the demoted manager: a grant again, refused.
    let mut restored = payload(&p.db, "user_scope_assignments", "to-revoke").await;
    restored.insert("deleted_at".into(), Value::Null);
    let outcome = receive(&p.db, "user_scope_assignments", "to-revoke", "upsert", restored).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");

    // An end date brought forward, and a delete: applied.
    assign(&p.db, "to-shorten", p.worker, p.operator, Some(p.manager), &now()).await;
    let mut shortened = payload(&p.db, "user_scope_assignments", "to-shorten").await;
    shortened.insert("valid_to".into(), stamp(chrono::Duration::days(3)).into());
    let outcome = receive(
        &p.db,
        "user_scope_assignments",
        "to-shorten",
        "upsert",
        shortened.clone(),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let outcome = receive(&p.db, "user_scope_assignments", "to-shorten", "delete", shortened).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A role permission received new: its author holds `adm.roles` here; a system role's permission
/// without an author (migrations, seeder) is applied.
#[tokio::test]
async fn a_role_permission_is_checked_against_its_author() {
    let p = plant().await;
    let layout = crate::sync::row_layout::layout(&p.db, "role_permissions")
        .await
        .expect("layout");
    // The wire format marker every payload of this version carries.
    let format = payload(&p.db, "user_accounts", "worker-sync")
        .await
        .get("_sync_format")
        .cloned()
        .expect("format marker");
    let permission = |role: &str, by: Option<&str>| {
        let mut row = Payload::new();
        row.insert("_sync_format".into(), format.clone());
        row.insert("role_sync_id".into(), role.into());
        row.insert("permission_key".into(), "ot.edit".into());
        row.insert("granted_at".into(), now().into());
        row.insert(
            "granted_by_sync_id".into(),
            by.map_or(Value::Null, |b| Value::String(b.to_string())),
        );
        row
    };
    let send = |row: Payload| {
        let sync_id = crate::sync::row_codec::natural_entity_sync_id(&layout, &row).expect("natural key");
        let db = &p.db;
        async move { receive(db, "role_permissions", &sync_id, "upsert", row).await }
    };
    let custom = "field-operators-sync";

    let outcome = send(permission(custom, None)).await;
    assert!(is_grant_conflict(&outcome), "no author: {outcome:?}");

    let supervisor = role_sync(&p.db, "Supervisor").await;
    let outcome = send(permission(&supervisor, None)).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "system role: {outcome:?}"
    );

    demote_manager(&p).await;
    let outcome = send(permission(custom, Some("manager-sync"))).await;
    assert!(is_grant_conflict(&outcome), "demoted author: {outcome:?}");

    let outcome = send(permission(custom, Some("chef-sync"))).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied),
        "plant administrator: {outcome:?}"
    );
}

#[tokio::test]
async fn the_bootstrap_flag_set_on_an_existing_account_is_a_conflict() {
    let p = plant().await;
    let mut row = payload(&p.db, "user_accounts", "worker-sync").await;
    let flag = match row.get("is_admin") {
        Some(Value::Bool(_)) => Value::Bool(true),
        _ => Value::from(1),
    };
    row.insert("is_admin".into(), flag);
    let outcome = receive(&p.db, "user_accounts", "worker-sync", "upsert", row).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");
    assert_eq!(
        scalar(
            &p.db,
            "SELECT is_admin FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        Some(0)
    );
}

/// On the PC where the manager kept giving access after the withdrawal made elsewhere: the
/// withdrawal is applied and the administrators get one notice listing the grants made since.
#[tokio::test]
async fn a_received_withdrawal_lists_the_grants_made_since_in_one_notice() {
    let p = plant().await;
    let withdrawn_at = stamp(chrono::Duration::hours(-2));
    let early = user(&p.db, "early-hire").await;
    let late = user(&p.db, "late-hire").await;
    assign(
        &p.db,
        "before-withdrawal",
        early,
        p.operator,
        Some(p.manager),
        &stamp(chrono::Duration::days(-2)),
    )
    .await;
    assign(
        &p.db,
        "after-withdrawal",
        late,
        p.operator,
        Some(p.manager),
        &stamp(chrono::Duration::hours(-1)),
    )
    .await;

    let mut withdrawal = payload(&p.db, "user_scope_assignments", &p.manager_assignment).await;
    withdrawal.insert("deleted_at".into(), withdrawn_at.into());
    let outcome = receive(
        &p.db,
        "user_scope_assignments",
        &p.manager_assignment,
        "upsert",
        withdrawal,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(
        assignment_held(&p.db, "after-withdrawal").await,
        "nothing ended silently"
    );

    let rows =
        p.db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(message_params_json, '') AS body FROM notification_events \
              WHERE message_key = 'admin_grants_to_review'"
                .to_string(),
        ))
        .await
        .expect("events");
    assert_eq!(rows.len(), 1, "one notice");
    let body: String = rows[0].try_get("", "body").expect("body");
    assert!(body.contains("late-hire"), "{body}");
    assert!(!body.contains("early-hire"), "{body}");
    let recipients = scalar(
        &p.db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
          WHERE e.message_key = 'admin_grants_to_review' AND n.recipient_user_id = ?",
        vec![p.chef.into()],
    )
    .await;
    assert_eq!(recipients, Some(1), "the plant administrator is told");
}
