//! Received grants re-checked against their author (`sync::grant_check`), through the real inbound
//! path (`apply_inbound_payload` → `row_codec::apply_row` → `apply_hooks::before_upsert_checked`),
//! with payloads read from the database as the codec sends them.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::rbac::permissions::{OT_CREATE, OT_EDIT, OT_VIEW};
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
    let operator = custom_role(&db, "field-operators", &[OT_VIEW]).await;
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
        row.insert("permission_key".into(), OT_EDIT.into());
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

// ── Review round 1 ──────────────────────────────────────────────────────────────────────────────

/// A received row waiting in the inbox, as the transport stores it.
async fn inbox(db: &DatabaseConnection, entity_type: &str, sync_id: &str, row: &Payload) {
    exec(
        db,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES (?, 'c1', ?, ?, 'upsert', 1, ?, ?, 'pending')",
        vec![
            format!("b-{sync_id}").into(),
            entity_type.into(),
            sync_id.into(),
            Value::Object(row.clone()).to_string().into(),
            format!("h-{sync_id}").into(),
        ],
    )
    .await;
}

async fn inbox_status(db: &DatabaseConnection, sync_id: &str) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = ?",
        [sync_id.into()],
    ))
    .await
    .unwrap()
    .expect("inbox row")
    .try_get::<String>("", "apply_status")
    .expect("status")
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    scalar(db, sql, vec![]).await.unwrap_or(0)
}

/// The wire form of an assignment made on another PC at `created_at`.
async fn remote_assignment_at(
    db: &DatabaseConnection,
    sync_id: &str,
    user: i64,
    role: i64,
    by: Option<i64>,
    created_at: &str,
) -> Payload {
    assign(db, sync_id, user, role, by, created_at).await;
    let row = payload(db, "user_scope_assignments", sync_id).await;
    exec(
        db,
        "DELETE FROM user_scope_assignments WHERE entity_sync_id = ?",
        vec![sync_id.into()],
    )
    .await;
    row
}

/// The first pull of a new PC replays old grants whose authors lost the right since: an author
/// disabled since (loss known here: grants made before it apply, nothing to review), an author whose
/// role lost the right (a role permission deleted: loss unknown here, grants apply and are listed
/// in ONE notice). No conflict.
#[tokio::test]
async fn a_first_pull_replays_old_grants_of_authors_who_lost_the_right() {
    let p = plant().await;
    let three_days_ago = stamp(chrono::Duration::days(-3));
    let managers = role_id(&p.db, "user-managers").await;
    let former = user(&p.db, "former").await;
    assign(&p.db, "former-role", former, managers, Some(p.chef), &three_days_ago).await;
    // Disabled since, through the command: its ledger records when (`user_deactivated`).
    crate::commands::admin_users::set_user_active_impl(
        &crate::state::AppState::new(p.db.clone()),
        &caller(p.chef, "chef"),
        crate::commands::admin_users::SetUserActiveInput {
            user_id: former,
            reason: "Left the plant".into(),
        },
        false,
    )
    .await
    .expect("disabled");
    let legacy = user(&p.db, "legacy").await;
    let old_managers = custom_role(&p.db, "old-managers", &[OT_VIEW]).await;
    assign(
        &p.db,
        "legacy-role",
        legacy,
        old_managers,
        Some(p.chef),
        &three_days_ago,
    )
    .await;

    for (sync_id, who, by) in [
        ("old-by-former-1", "hire-1", former),
        ("old-by-former-2", "hire-2", former),
        ("old-by-legacy-1", "hire-3", legacy),
        ("old-by-legacy-2", "hire-4", legacy),
    ] {
        let hire = user(&p.db, who).await;
        let row = remote_assignment_at(&p.db, sync_id, hire, p.operator, Some(by), &three_days_ago).await;
        inbox(&p.db, "user_scope_assignments", sync_id, &row).await;
    }
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");

    for sync_id in [
        "old-by-former-1",
        "old-by-former-2",
        "old-by-legacy-1",
        "old-by-legacy-2",
    ] {
        assert!(assignment_held(&p.db, sync_id).await, "{sync_id} applied");
    }
    assert_eq!(
        count(&p.db, "SELECT COUNT(*) FROM sync_conflicts").await,
        0,
        "no conflict"
    );
    let notices =
        p.db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(message_params_json, '') AS params FROM notification_events \
              WHERE message_key = 'grants_from_former_authors'"
                .to_string(),
        ))
        .await
        .expect("events");
    assert_eq!(notices.len(), 1, "one review notice");
    let params: String = notices[0].try_get("", "params").expect("params");
    assert!(params.contains("hire-3 :") && params.contains("hire-4 :"), "{params}");
    assert!(!params.contains("hire-1 :"), "known loss, made before: {params}");
}

/// An author never received waits while sync catches up; after a complete pull it waits no longer
/// (applied, listed for review). Two grants whose authors' rights wait on each other end too.
#[tokio::test]
async fn waits_end_after_a_complete_pull() {
    let p = plant().await;
    let mut ghost = remote_assignment(&p.db, "by-ghost", p.worker, p.operator, Some(p.manager)).await;
    ghost.insert("assigned_by_sync_id".into(), "ghost-sync".into());
    inbox(&p.db, "user_scope_assignments", "by-ghost", &ghost).await;

    let managers = role_id(&p.db, "user-managers").await;
    let x = user(&p.db, "mutual-x").await;
    let y = user(&p.db, "mutual-y").await;
    let x_row = remote_assignment(&p.db, "x-by-y", x, managers, Some(y)).await;
    let y_row = remote_assignment(&p.db, "y-by-x", y, managers, Some(x)).await;
    inbox(&p.db, "user_scope_assignments", "x-by-y", &x_row).await;
    inbox(&p.db, "user_scope_assignments", "y-by-x", &y_row).await;

    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    for sync_id in ["by-ghost", "x-by-y", "y-by-x"] {
        assert_eq!(inbox_status(&p.db, sync_id).await, "deferred", "{sync_id} waits");
    }

    crate::sync::grant_check::release_after_complete_pull(&p.db)
        .await
        .expect("release");
    assert!(assignment_held(&p.db, "by-ghost").await, "applied for review");
    assert_eq!(
        count(
            &p.db,
            "SELECT COUNT(*) FROM notification_events WHERE message_key = 'grants_from_former_authors'"
        )
        .await,
        1
    );
    // Neither author ever held the right here: both end as conflicts, for an administrator.
    for sync_id in ["x-by-y", "y-by-x"] {
        assert_eq!(inbox_status(&p.db, sync_id).await, "conflict", "{sync_id}");
    }
    // The author never received stays named: a local change of the row sends it again.
    let sent = payload(&p.db, "user_scope_assignments", "by-ghost").await;
    assert_eq!(
        sent.get("assigned_by_sync_id").and_then(Value::as_str),
        Some("ghost-sync"),
        "{sent:?}"
    );
}

/// An author whose rights are still in the inbox: the grant waits for them.
#[tokio::test]
async fn a_grant_waits_while_its_authors_rights_are_still_arriving() {
    let p = plant().await;
    let managers = role_id(&p.db, "user-managers").await;
    let newcomer = user(&p.db, "newcomer").await;
    let rights = remote_assignment(&p.db, "newcomer-role", newcomer, managers, Some(p.chef)).await;
    inbox(&p.db, "user_scope_assignments", "newcomer-role", &rights).await;
    let row = remote_assignment(&p.db, "by-newcomer", p.worker, p.operator, Some(newcomer)).await;
    let outcome = receive(&p.db, "user_scope_assignments", "by-newcomer", "upsert", row).await;
    assert!(
        matches!(outcome, ApplyOutcome::Deferred { ref waits_for, .. } if waits_for.is_none()),
        "{outcome:?}"
    );
}

/// Widening a held assignment is a grant: validity extended, scope changed, emergency made
/// permanent, each by a demoted author, is a conflict.
#[tokio::test]
async fn widening_a_held_assignment_is_checked() {
    let p = plant().await;
    assign(&p.db, "widen", p.worker, p.operator, Some(p.manager), &now()).await;
    exec(
        &p.db,
        "UPDATE user_scope_assignments SET valid_to = ? WHERE entity_sync_id = 'widen'",
        vec![stamp(chrono::Duration::days(3)).into()],
    )
    .await;
    let night_role = custom_role(&p.db, "night-cover", &[OT_VIEW]).await;
    exec(
        &p.db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, assigned_by_id, \
         is_emergency, emergency_reason, emergency_expires_at, created_at, updated_at, row_version) \
         VALUES ('emergency', ?, ?, 'tenant', ?, 1, 'night shift', ?, ?, ?, 1)",
        vec![
            p.worker.into(),
            night_role.into(),
            p.manager.into(),
            stamp(chrono::Duration::hours(4)).into(),
            now().into(),
            now().into(),
        ],
    )
    .await;
    let held = payload(&p.db, "user_scope_assignments", "widen").await;
    let emergency = payload(&p.db, "user_scope_assignments", "emergency").await;
    demote_manager(&p).await;

    let mut longer = held.clone();
    longer.insert("valid_to".into(), stamp(chrono::Duration::days(300)).into());
    let outcome = receive(&p.db, "user_scope_assignments", "widen", "upsert", longer).await;
    assert!(is_grant_conflict(&outcome), "validity: {outcome:?}");

    let mut moved = held;
    moved.insert("scope_type".into(), "site".into());
    moved.insert("scope_reference_sync_id".into(), "some-site".into());
    let outcome = receive(&p.db, "user_scope_assignments", "widen", "upsert", moved).await;
    assert!(is_grant_conflict(&outcome), "scope: {outcome:?}");

    let mut permanent = emergency;
    let off = match permanent.get("is_emergency") {
        Some(Value::Bool(_)) => Value::Bool(false),
        _ => Value::from(0),
    };
    permanent.insert("is_emergency".into(), off);
    let outcome = receive(&p.db, "user_scope_assignments", "emergency", "upsert", permanent).await;
    assert!(is_grant_conflict(&outcome), "emergency made permanent: {outcome:?}");
}

/// "Take the received version" in the Sync centre runs the check again: still refused.
#[tokio::test]
async fn accepting_the_remote_version_runs_the_check_again() {
    let p = plant().await;
    let row = remote_assignment(&p.db, "remote-late", p.worker, p.operator, Some(p.manager)).await;
    demote_manager(&p).await;
    inbox(&p.db, "user_scope_assignments", "remote-late", &row).await;
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    let conflict =
        p.db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, row_version FROM sync_conflicts WHERE conflict_type = ? AND entity_sync_id = 'remote-late'",
            [GRANT_BY_UNAUTHORISED_ADMIN.into()],
        ))
        .await
        .unwrap()
        .expect("conflict recorded");
    let result = crate::sync::queries::resolve_sync_conflict(
        &p.db,
        p.chef,
        crate::sync::domain::ResolveSyncConflictInput {
            conflict_id: conflict.try_get("", "id").unwrap(),
            expected_row_version: conflict.try_get("", "row_version").unwrap(),
            action: "accept_remote".into(),
            resolution_note: Some("Taking the version from the other PC".into()),
        },
    )
    .await;
    assert!(result.is_err(), "{result:?}");
    assert!(!assignment_held(&p.db, "remote-late").await);
}

/// An account holding administration rights re-enabled or restored on another PC (the row names
/// nobody) is a conflict; a plain account is re-enabled.
#[tokio::test]
async fn re_enabling_an_account_with_administration_rights_is_a_conflict() {
    let p = plant().await;
    let flag = |row: &Payload, on: bool| match row.get("is_active") {
        Some(Value::Bool(_)) => Value::Bool(on),
        _ => Value::from(i64::from(on)),
    };
    let mut enabled = payload(&p.db, "user_accounts", "manager-sync").await;
    let on = flag(&enabled, true);
    enabled.insert("is_active".into(), on);
    exec(
        &p.db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![p.manager.into()],
    )
    .await;
    let outcome = receive(&p.db, "user_accounts", "manager-sync", "upsert", enabled.clone()).await;
    assert!(is_grant_conflict(&outcome), "re-enabled: {outcome:?}");

    exec(
        &p.db,
        "UPDATE user_accounts SET is_active = 1, deleted_at = ? WHERE id = ?",
        vec![now().into(), p.manager.into()],
    )
    .await;
    let mut restored = enabled;
    restored.insert("deleted_at".into(), Value::Null);
    let outcome = receive(&p.db, "user_accounts", "manager-sync", "upsert", restored).await;
    assert!(is_grant_conflict(&outcome), "restored: {outcome:?}");

    let mut worker = payload(&p.db, "user_accounts", "worker-sync").await;
    let on = flag(&worker, true);
    worker.insert("is_active".into(), on);
    // Re-enabled by the plant administrator: a re-enable names its author (`sync::credential_check`).
    worker.insert("status_changed_by_sync_id".into(), "chef-sync".into());
    exec(
        &p.db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![p.worker.into()],
    )
    .await;
    let outcome = receive(&p.db, "user_accounts", "worker-sync", "upsert", worker).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "plain account: {outcome:?}");
    assert_eq!(
        scalar(
            &p.db,
            "SELECT is_active FROM user_accounts WHERE id = ?",
            vec![p.worker.into()]
        )
        .await,
        Some(1),
        "re-enabled"
    );
}

/// A new account with the bootstrap flag: received on a PC joining the plant (no first
/// administrator here yet) it applies; on a PC that has its first administrator it is a conflict.
#[tokio::test]
async fn a_new_bootstrap_account_is_refused_once_this_pc_has_its_first_administrator() {
    let p = plant().await;
    let first = user(&p.db, "first-elsewhere").await;
    exec(
        &p.db,
        "UPDATE user_accounts SET is_admin = 1 WHERE id = ?",
        vec![first.into()],
    )
    .await;
    let row = payload(&p.db, "user_accounts", "first-elsewhere-sync").await;
    exec(&p.db, "DELETE FROM user_accounts WHERE id = ?", vec![first.into()]).await;

    // Joining the plant: no first administrator recorded here.
    assert!(!crate::auth::builtin_admin::first_admin_marker_closes(&p.db)
        .await
        .unwrap());
    let outcome = receive(&p.db, "user_accounts", "first-elsewhere-sync", "upsert", row.clone()).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "join: {outcome:?}");
    exec(
        &p.db,
        "DELETE FROM user_accounts WHERE entity_sync_id = 'first-elsewhere-sync'",
        vec![],
    )
    .await;

    crate::auth::builtin_admin::record_first_administrator_in(&p.db, "chef-sync")
        .await
        .expect("marker");
    let outcome = receive(&p.db, "user_accounts", "first-elsewhere-sync", "upsert", row).await;
    assert!(is_grant_conflict(&outcome), "second first administrator: {outcome:?}");
}

// ── Review round 2 ──────────────────────────────────────────────────────────────────────────────

fn caller(user_id: i64, username: &str) -> crate::auth::session_manager::AuthenticatedUser {
    crate::auth::session_manager::AuthenticatedUser {
        user_id: i32::try_from(user_id).expect("fits"),
        username: username.into(),
        display_name: None,
        is_admin: false,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    }
}

async fn review_notices(db: &DatabaseConnection) -> Vec<String> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT COALESCE(message_params_json, '') AS params FROM notification_events \
          WHERE message_key = 'grants_from_former_authors' ORDER BY id"
            .to_string(),
    ))
    .await
    .expect("events")
    .iter()
    .map(|r| r.try_get::<String>("", "params").expect("params"))
    .collect()
}

fn minus(at: chrono::DateTime<chrono::Utc>, minutes: i64) -> String {
    (at - chrono::Duration::minutes(minutes))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// A disabled author: the cutoff is the disable recorded by its ledger (`user_deactivated`), not
/// the account's `updated_at` that a later edit moves; the clock skew boundary holds on both sides.
/// Disabled with no record of it here: not known, the grant is applied for review.
#[tokio::test]
async fn a_disabled_authors_cutoff_is_the_recorded_disable_time() {
    let p = plant().await;
    let state = crate::state::AppState::new(p.db.clone());
    let chef = caller(p.chef, "chef");
    crate::commands::admin_users::set_user_active_impl(
        &state,
        &chef,
        crate::commands::admin_users::SetUserActiveInput {
            user_id: p.manager,
            reason: "Left the plant".into(),
        },
        false,
    )
    .await
    .expect("disabled");
    let disabled_at: String =
        p.db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT acted_at FROM admin_change_events WHERE action = 'user_deactivated' AND target_user_id = ?",
            [p.manager.into()],
        ))
        .await
        .unwrap()
        .expect("ledger")
        .try_get("", "acted_at")
        .unwrap();
    let disabled_at = crate::settings::site::instant(&disabled_at).expect("time");
    // A later edit of the account moves `updated_at` only.
    exec(
        &p.db,
        "UPDATE user_accounts SET display_name = 'Former manager', updated_at = ? WHERE id = ?",
        vec![stamp(chrono::Duration::days(2)).into(), p.manager.into()],
    )
    .await;

    for (sync_id, minutes) in [
        ("well-before", 120),
        ("just-before-skew", 11),
        ("within-skew", 9),
        ("after-disable", -60),
    ] {
        let hire = user(&p.db, &format!("hire-{sync_id}")).await;
        let row = remote_assignment_at(
            &p.db,
            sync_id,
            hire,
            p.operator,
            Some(p.manager),
            &minus(disabled_at, minutes),
        )
        .await;
        inbox(&p.db, "user_scope_assignments", sync_id, &row).await;
    }
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    for (sync_id, applies) in [
        ("well-before", true),
        ("just-before-skew", true),
        ("within-skew", false),
        ("after-disable", false),
    ] {
        let status = inbox_status(&p.db, sync_id).await;
        assert_eq!(status == "applied", applies, "{sync_id}: {status}");
        if !applies {
            assert_eq!(status, "conflict", "{sync_id}");
        }
    }
    assert!(review_notices(&p.db).await.is_empty(), "known loss: nothing to review");

    // Disabled with no ledger record here: the loss is not known.
    let quiet = user(&p.db, "quiet-manager").await;
    let managers = role_id(&p.db, "user-managers").await;
    assign(&p.db, "quiet-role", quiet, managers, Some(p.chef), &now()).await;
    exec(
        &p.db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![quiet.into()],
    )
    .await;
    let row = remote_assignment(&p.db, "by-quiet", p.worker, p.operator, Some(quiet)).await;
    inbox(&p.db, "user_scope_assignments", "by-quiet", &row).await;
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    assert!(assignment_held(&p.db, "by-quiet").await);
    let notices = review_notices(&p.db).await;
    assert_eq!(notices.len(), 1, "{notices:?}");
    // Readable: the person, the role, who gave it, the day only.
    let params: Value = serde_json::from_str(&notices[0]).expect("params");
    let grants = params.get("grants").and_then(Value::as_str).unwrap_or_default();
    assert!(
        grants.contains("worker") && grants.contains("quiet-manager"),
        "{grants}"
    );
    assert!(
        grants.contains(&chrono::Utc::now().format("%Y-%m-%d").to_string()) && !grants.contains('Z'),
        "a day, no timestamp: {grants}"
    );
}

/// After a complete pull, a grant by B waits for B's own role still waiting (its author never
/// received): the role applies for review first, then the grant applies (a fixpoint).
#[tokio::test]
async fn after_a_complete_pull_a_grant_waits_for_its_authors_own_role() {
    let p = plant().await;
    let managers = role_id(&p.db, "user-managers").await;
    let b = user(&p.db, "author-b").await;
    let mut b_role = remote_assignment(&p.db, "b-role", b, managers, Some(p.chef)).await;
    b_role.insert("assigned_by_sync_id".into(), "ghost-sync".into());
    let by_b = remote_assignment(&p.db, "by-b", p.worker, p.operator, Some(b)).await;
    // The grant first: judged before the role in every sweep.
    inbox(&p.db, "user_scope_assignments", "by-b", &by_b).await;
    inbox(&p.db, "user_scope_assignments", "b-role", &b_role).await;
    crate::sync::materialize::materialize_inbox(&p.db, 200, true)
        .await
        .expect("materialize");
    assert_eq!(inbox_status(&p.db, "by-b").await, "deferred");

    crate::sync::grant_check::release_after_complete_pull(&p.db)
        .await
        .expect("release");
    assert_eq!(inbox_status(&p.db, "b-role").await, "applied");
    assert_eq!(inbox_status(&p.db, "by-b").await, "applied");
    assert_eq!(review_notices(&p.db).await.len(), 1, "one notice for the run");
}

/// A widening that waits after the check (its team not received) lists nothing, however often it
/// is tried; a notice that cannot be written leaves the grant waiting (fail closed), then it applies.
#[tokio::test]
async fn a_grant_for_review_is_listed_once_and_only_when_applied() {
    let p = plant().await;
    let legacy = user(&p.db, "legacy").await;
    let old_managers = custom_role(&p.db, "old-managers", &[OT_VIEW]).await;
    assign(&p.db, "legacy-role", legacy, old_managers, Some(p.chef), &now()).await;
    assign(&p.db, "held", p.worker, p.operator, Some(legacy), &now()).await;
    let mut widened = payload(&p.db, "user_scope_assignments", "held").await;
    widened.insert("scope_type".into(), "team".into());
    widened.insert("scope_reference_sync_id".into(), "team-not-here".into());
    // Tried on three synchronisations: it waits each time (the team), and its line goes with it
    // (a row not applied is rolled back with everything its apply wrote, as the inbox does).
    for _ in 0..3 {
        use sea_orm::TransactionTrait;
        let tx = p.db.begin().await.expect("begin");
        let outcome = apply_inbound_payload(
            &tx,
            "user_scope_assignments",
            "held",
            "upsert",
            2,
            &Value::Object(widened.clone()).to_string(),
        )
        .await
        .expect("apply");
        tx.rollback().await.expect("rollback");
        assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    }
    assert!(
        review_notices(&p.db).await.is_empty(),
        "nothing applied, nothing listed"
    );

    exec(
        &p.db,
        "CREATE TRIGGER refuse_review_notice BEFORE INSERT ON notification_events \
         WHEN NEW.message_key = 'grants_from_former_authors' BEGIN SELECT RAISE(ABORT, 'disk full'); END",
        vec![],
    )
    .await;
    let newcomer = user(&p.db, "newcomer").await;
    let row = remote_assignment(&p.db, "new-by-legacy", newcomer, p.operator, Some(legacy)).await;
    let outcome = receive(&p.db, "user_scope_assignments", "new-by-legacy", "upsert", row.clone()).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert!(!assignment_held(&p.db, "new-by-legacy").await);
    exec(&p.db, "DROP TRIGGER refuse_review_notice", vec![]).await;
    let outcome = receive(&p.db, "user_scope_assignments", "new-by-legacy", "upsert", row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(review_notices(&p.db).await.len(), 1);
}

/// Role permissions: an author whose role lost the right (loss unknown) gives it for review; an
/// author who never held `adm.roles` here is refused.
#[tokio::test]
async fn role_permission_review_and_never_held() {
    let p = plant().await;
    let legacy = user(&p.db, "legacy").await;
    let old_managers = custom_role(&p.db, "old-managers", &[OT_VIEW]).await;
    assign(&p.db, "legacy-role", legacy, old_managers, Some(p.chef), &now()).await;
    let layout = crate::sync::row_layout::layout(&p.db, "role_permissions")
        .await
        .expect("layout");
    let format = payload(&p.db, "user_accounts", "worker-sync")
        .await
        .get("_sync_format")
        .cloned()
        .expect("format marker");
    let permission = |name: &str, by: &str| {
        let mut row = Payload::new();
        row.insert("_sync_format".into(), format.clone());
        row.insert("role_sync_id".into(), "field-operators-sync".into());
        row.insert("permission_key".into(), name.into());
        row.insert("granted_at".into(), now().into());
        row.insert("granted_by_sync_id".into(), by.into());
        row
    };
    let send = |row: Payload| {
        let sync_id = crate::sync::row_codec::natural_entity_sync_id(&layout, &row).expect("natural key");
        let db = &p.db;
        async move { receive(db, "role_permissions", &sync_id, "upsert", row).await }
    };
    let outcome = send(permission(OT_EDIT, "legacy-sync")).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "loss unknown: {outcome:?}");
    let notices = review_notices(&p.db).await;
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].contains(OT_EDIT) && notices[0].contains("legacy"),
        "{}",
        notices[0]
    );

    let outcome = send(permission(OT_CREATE, "worker-sync")).await;
    assert!(is_grant_conflict(&outcome), "never held: {outcome:?}");
}

/// Re-enabling an administration account through `reactivate_user` on another PC is a conflict
/// here. `update_user` cannot re-enable an account (`GATE_ADMIN_USE_DISABLE`): a details edit of a
/// disabled one is applied, still disabled.
#[tokio::test]
async fn reactivating_through_the_commands_is_checked() {
    let p = plant().await;
    let state = crate::state::AppState::new(p.db.clone());
    let chef = caller(p.chef, "chef");
    let input = |user_id| crate::commands::admin_users::SetUserActiveInput {
        user_id,
        reason: "Back at the plant".into(),
    };
    crate::commands::admin_users::set_user_active_impl(&state, &chef, input(p.manager), false)
        .await
        .expect("disabled");
    crate::commands::admin_users::set_user_active_impl(&state, &chef, input(p.manager), true)
        .await
        .expect("re-enabled on the other PC");
    let enabled = payload(&p.db, "user_accounts", "manager-sync").await;
    crate::commands::admin_users::set_user_active_impl(&state, &chef, input(p.manager), false)
        .await
        .expect("disabled here");
    let outcome = receive(&p.db, "user_accounts", "manager-sync", "upsert", enabled).await;
    assert!(is_grant_conflict(&outcome), "{outcome:?}");

    let refused = crate::commands::admin_users::update_user_impl(
        &state,
        &chef,
        crate::commands::admin_users::UpdateUserInput {
            user_id: p.manager,
            username: None,
            display_name: None,
            email: None,
            phone: None,
            personnel_id: None,
            force_password_change: None,
            is_active: Some(true),
        },
    )
    .await;
    assert!(refused.is_err(), "update_user never re-enables: {refused:?}");
    let mut edited = payload(&p.db, "user_accounts", "manager-sync").await;
    edited.insert("display_name".into(), "Renamed elsewhere".into());
    let outcome = receive(&p.db, "user_accounts", "manager-sync", "upsert", edited).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}
