//! Admin user / role lifecycle: filters, reasons, scope validation and the admin change ledger.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::*;
use crate::state::AppState;

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "v")
        .expect("decode")
}

async fn admin_caller(db: &DatabaseConnection) -> AuthenticatedUser {
    let id = scalar_i64(db, "SELECT id AS v FROM user_accounts WHERE username = 'admin'", vec![]).await;
    AuthenticatedUser {
        user_id: i32::try_from(id).expect("fits"),
        username: "admin".into(),
        display_name: None,
        is_admin: true,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    }
}

async fn insert_user(db: &DatabaseConnection, username: &str) -> i64 {
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts \
         (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES (?, ?, 'local', 'x', 1, 0, 0, 0, ?, ?, 1)",
        [
            format!("{username}-sync").into(),
            username.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .expect("insert user");
    scalar_i64(
        db,
        "SELECT id AS v FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await
}

async fn role_id(db: &DatabaseConnection, name: &str) -> i64 {
    scalar_i64(db, "SELECT id AS v FROM roles WHERE name = ?", vec![name.into()]).await
}

/// Site 1 with an area 2 below it (active structure model).
async fn seed_org(db: &DatabaseConnection) {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (901, 'adm-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
         VALUES (901, 'adm-nt', 901, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, structure_model_id, code, name, node_type_id, status, created_at, updated_at) \
         VALUES (901, 'adm-site', 901, 'SITE-A', 'Site A', 901, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, structure_model_id, code, name, node_type_id, parent_id, status, created_at, updated_at) \
         VALUES (902, 'adm-area', 901, 'AREA-A1', 'Area A1', 901, 901, 'active', datetime('now'), datetime('now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("seed org");
    }
}

async fn ledger_count(db: &DatabaseConnection, action: &str, result: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) AS v FROM admin_change_events WHERE action = ? AND apply_result = ?",
        vec![action.into(), result.into()],
    )
    .await
}

fn assign(user_id: i64, role_id: i64, scope_type: &str, reference: Option<&str>) -> AssignRoleScopeInput {
    AssignRoleScopeInput {
        user_id,
        role_id,
        scope_type: scope_type.into(),
        scope_reference: reference.map(str::to_string),
        valid_from: None,
        valid_to: None,
    }
}

#[tokio::test]
async fn scoped_assignment_needs_an_existing_node_and_a_valid_window() {
    let db = crate::db::test_db::seeded().await;
    seed_org(&db).await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let user = insert_user(&db, "scope_user").await;
    let readonly = role_id(&db, "Readonly").await;

    let missing_ref =
        assign_role_scope_impl(&state, &caller, assign(user, readonly, "org_node", None), false, None).await;
    assert!(matches!(missing_ref, Err(AppError::ValidationFailed(_))));
    let unknown_node = assign_role_scope_impl(
        &state,
        &caller,
        assign(user, readonly, "site", Some("424242")),
        false,
        None,
    )
    .await;
    assert!(matches!(unknown_node, Err(AppError::ValidationFailed(_))));
    let bad_type = assign_role_scope_impl(&state, &caller, assign(user, readonly, "global", None), false, None).await;
    assert!(matches!(bad_type, Err(AppError::ValidationFailed(_))));

    let mut reversed = assign(user, readonly, "org_node", Some("901"));
    reversed.valid_from = Some("2026-12-31".into());
    reversed.valid_to = Some("2026-01-01".into());
    assert!(assign_role_scope_impl(&state, &caller, reversed, false, None)
        .await
        .is_err());

    let mut ok = assign(user, readonly, "org_node", Some("901"));
    ok.valid_from = Some("2026-01-01".into());
    ok.valid_to = Some("2099-12-31".into());
    assign_role_scope_impl(&state, &caller, ok, false, None)
        .await
        .expect("assignment on an existing node");
    let stored_to: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT valid_to FROM user_scope_assignments WHERE user_id = ? AND deleted_at IS NULL",
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "valid_to")
        .unwrap();
    assert_eq!(stored_to, "2099-12-31T23:59:59Z", "an end date covers the whole day");

    let roles = load_role_assignments(&db, user).await.unwrap();
    assert_eq!(roles[0].scope_label.as_deref(), Some("Site A"));

    let nodes = list_assignable_scope_nodes_impl(&db).await.unwrap();
    let ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
    assert!(
        ids.contains(&901) && ids.contains(&902),
        "nodes of the active structure are offered"
    );
}

#[tokio::test]
async fn user_list_filters_by_status_role_and_site_subtree() {
    let db = crate::db::test_db::seeded().await;
    seed_org(&db).await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let readonly = role_id(&db, "Readonly").await;
    let on_area = insert_user(&db, "filter_area").await;
    let locked = insert_user(&db, "filter_locked").await;
    let stale_lock = insert_user(&db, "filter_stale_lock").await;

    assign_role_scope_impl(
        &state,
        &caller,
        assign(on_area, readonly, "org_node", Some("902")),
        false,
        None,
    )
    .await
    .unwrap();
    let future = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let past = (chrono::Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();
    for (id, until) in [(locked, future), (stale_lock, past)] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET locked_until = ? WHERE id = ?",
            [until.into(), id.into()],
        ))
        .await
        .unwrap();
    }

    let site_users = list_users_impl(
        &db,
        &UserListFilter {
            is_active: None,
            status: None,
            identity_mode: None,
            search: None,
            role_id: None,
            org_node_id: Some(901),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        site_users.iter().map(|u| u.id).collect::<Vec<_>>(),
        vec![on_area],
        "a user assigned on an area appears under its site"
    );

    let by_role = list_users_impl(
        &db,
        &UserListFilter {
            is_active: None,
            status: None,
            identity_mode: None,
            search: Some("filter_".into()),
            role_id: Some(readonly),
            org_node_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(by_role.iter().map(|u| u.id).collect::<Vec<_>>(), vec![on_area]);

    let locked_users = list_users_impl(
        &db,
        &UserListFilter {
            is_active: None,
            status: Some("locked".into()),
            identity_mode: None,
            search: None,
            role_id: None,
            org_node_id: None,
        },
    )
    .await
    .unwrap();
    let ids: Vec<i64> = locked_users.iter().map(|u| u.id).collect();
    assert!(ids.contains(&locked));
    assert!(!ids.contains(&stale_lock), "an expired lockout is not a lock");
    assert!(locked_users.iter().all(|u| u.is_locked));
}

#[tokio::test]
async fn disabling_and_enabling_need_a_reason_and_are_ledgered() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let user = insert_user(&db, "lifecycle_user").await;

    let no_reason = set_user_active_impl(
        &state,
        &caller,
        SetUserActiveInput {
            user_id: user,
            reason: "   ".into(),
        },
        false,
    )
    .await;
    assert!(matches!(no_reason, Err(AppError::ValidationFailed(_))));

    let self_disable = set_user_active_impl(
        &state,
        &caller,
        SetUserActiveInput {
            user_id: i64::from(caller.user_id),
            reason: "test".into(),
        },
        false,
    )
    .await;
    assert!(self_disable.is_err(), "an admin cannot disable their own account");

    set_user_active_impl(
        &state,
        &caller,
        SetUserActiveInput {
            user_id: user,
            reason: "Left the company".into(),
        },
        false,
    )
    .await
    .expect("disable");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_active AS v FROM user_accounts WHERE id = ?",
            vec![user.into()]
        )
        .await,
        0
    );
    let summary: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT summary FROM admin_change_events WHERE action = 'user_deactivated' AND target_user_id = ?",
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "summary")
        .unwrap();
    assert_eq!(summary, "Left the company");

    let again = set_user_active_impl(
        &state,
        &caller,
        SetUserActiveInput {
            user_id: user,
            reason: "twice".into(),
        },
        false,
    )
    .await;
    assert!(again.is_err(), "disabling a disabled account is refused");

    set_user_active_impl(
        &state,
        &caller,
        SetUserActiveInput {
            user_id: user,
            reason: "Back from leave".into(),
        },
        true,
    )
    .await
    .expect("enable");
    assert_eq!(ledger_count(&db, "user_reactivated", "applied").await, 1);
}

#[tokio::test]
async fn refused_admin_actions_are_ledgered_as_blocked() {
    let db = crate::db::test_db::seeded().await;
    let actor = i64::from(admin_caller(&db).await.user_id);
    let target = insert_user(&db, "blocked_target").await;
    let result: AppResult<()> = Err(AppError::ValidationFailed(vec!["nope".into()]));
    crate::commands::admin_governance::record_blocked_admin_action(
        &db,
        actor,
        "user_deactivated",
        Some(target),
        None,
        &result,
    )
    .await;
    let step_up: AppResult<()> = Err(AppError::StepUpRequired);
    crate::commands::admin_governance::record_blocked_admin_action(
        &db,
        actor,
        "user_deactivated",
        Some(target),
        None,
        &step_up,
    )
    .await;
    assert_eq!(
        ledger_count(&db, "user_deactivated", "blocked").await,
        1,
        "a missing step-up is a prompt, not a refusal"
    );
}

#[tokio::test]
async fn emergency_grants_are_validated_ledgered_and_revocable() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let user = insert_user(&db, "emergency_user").await;
    let readonly = role_id(&db, "Readonly").await;
    let grant = |scope: &str, expires: String, reason: &str| GrantEmergencyElevationInput {
        user_id: user,
        role_id: readonly,
        scope_type: scope.into(),
        scope_reference: None,
        reason: reason.into(),
        expires_at: expires,
    };
    let in_one_hour = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let an_hour_ago = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();

    assert!(
        grant_emergency_elevation_impl(&state, &caller, grant("global", in_one_hour.clone(), "Line stop"))
            .await
            .is_err(),
        "'global' matches no permission check; only tenant or a real scope is accepted"
    );
    assert!(
        grant_emergency_elevation_impl(&state, &caller, grant("tenant", an_hour_ago, "Line stop"))
            .await
            .is_err()
    );
    assert!(
        grant_emergency_elevation_impl(&state, &caller, grant("tenant", in_one_hour.clone(), " "))
            .await
            .is_err()
    );

    let id = grant_emergency_elevation_impl(&state, &caller, grant("tenant", in_one_hour, "Line stop on press 3"))
        .await
        .expect("grant")
        .id;
    assert_eq!(ledger_count(&db, "emergency_grant_created", "applied").await, 1);
    let perms = resolver::effective_permissions(&db, user, "tenant", None)
        .await
        .unwrap();
    assert!(!perms.is_empty(), "the grant is effective at plant level");

    revoke_assignment_impl(&state, &caller, id, Some("Line restarted"), true)
        .await
        .expect("revoke");
    assert_eq!(ledger_count(&db, "emergency_grant_revoked", "applied").await, 1);
}

#[tokio::test]
async fn roles_are_cloned_updated_and_retired_with_a_diff() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let readonly = role_id(&db, "Readonly").await;
    let source_perms = load_role_permission_names(&db, readonly).await.unwrap();

    let copy = clone_role_impl(
        &state,
        &caller,
        CloneRoleInput {
            source_role_id: readonly,
            name: "Readonly (site B)".into(),
            description: Some("Copy for site B".into()),
        },
    )
    .await
    .expect("clone")
    .id;
    assert_eq!(load_role_permission_names(&db, copy).await.unwrap(), source_perms);
    let duplicate = clone_role_impl(
        &state,
        &caller,
        CloneRoleInput {
            source_role_id: readonly,
            name: "readonly (SITE B)".into(),
            description: None,
        },
    )
    .await;
    assert!(duplicate.is_err(), "role names are unique regardless of case");

    let removed = source_perms.first().cloned().expect("readonly has permissions");
    let changed = update_role_impl(
        &state,
        &caller,
        UpdateRoleInput {
            role_id: copy,
            description: Some("Site B viewers".into()),
            add_permissions: None,
            remove_permissions: Some(vec![removed.clone()]),
        },
    )
    .await;
    // Removing a permission another one depends on is refused by the hard-dependency rule;
    // either way nothing half-applied stays behind.
    if let Ok(perms_changed) = changed {
        assert!(perms_changed);
        let diff: String = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT diff_json FROM admin_change_events WHERE action = 'role_updated' AND target_role_id = ?",
                [copy.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "diff_json")
            .unwrap();
        assert!(diff.contains(&removed));
    } else {
        assert_eq!(load_role_permission_names(&db, copy).await.unwrap(), source_perms);
    }

    retire_role_impl(&state, &caller, copy)
        .await
        .expect("retire unused copy");
    assert_eq!(ledger_count(&db, "role_retired", "applied").await, 1);
    assert!(
        retire_role_impl(&state, &caller, readonly).await.is_err(),
        "system roles cannot be retired"
    );
}

async fn admin_assignment(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT usa.id AS v FROM user_scope_assignments usa \
         JOIN user_accounts ua ON ua.id = usa.user_id JOIN roles r ON r.id = usa.role_id \
         WHERE ua.username = 'admin' AND r.name = 'Administrator' AND usa.deleted_at IS NULL",
        vec![],
    )
    .await
}

#[tokio::test]
async fn the_last_administrator_cannot_be_removed_by_any_path() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let admin_role = role_id(&db, "Administrator").await;
    let readonly = role_id(&db, "Readonly").await;
    let only = admin_assignment(&db).await;

    // Withdrawing the only administrator assignment is refused and rolled back.
    let revoked = revoke_assignment_impl(&state, &caller, only, None, false).await;
    assert!(matches!(revoked, Err(AppError::ValidationFailed(_))), "{revoked:?}");
    assert_eq!(admin_assignment(&db).await, only, "nothing was withdrawn");

    // Replacing it at the same scope is refused too (and an own account is never re-assigned).
    let replaced = assign_role_scope_impl(
        &state,
        &caller,
        assign(i64::from(caller.user_id), readonly, "tenant", None),
        false,
        None,
    )
    .await;
    assert!(replaced.is_err(), "{replaced:?}");
    assert_eq!(admin_assignment(&db).await, only);

    // An emergency administrator does not count as a remaining administrator.
    let helper = insert_user(&db, "emergency_admin").await;
    grant_emergency_elevation_impl(
        &state,
        &caller,
        GrantEmergencyElevationInput {
            user_id: helper,
            role_id: admin_role,
            scope_type: "tenant".into(),
            scope_reference: None,
            reason: "Night shift cover".into(),
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339(),
        },
    )
    .await
    .expect("grant");
    assert!(revoke_assignment_impl(&state, &caller, only, None, false)
        .await
        .is_err());

    // With a second regular administrator the first one can be withdrawn.
    assign_role_scope_impl(&state, &caller, assign(helper, admin_role, "tenant", None), false, None)
        .await
        .expect("second administrator");
    revoke_assignment_impl(&state, &caller, only, Some("Handover"), false)
        .await
        .expect("no longer the last one");
}

#[tokio::test]
async fn replacing_a_role_at_the_same_scope_ledgers_the_replaced_one() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let user = insert_user(&db, "replace_user").await;
    let readonly = role_id(&db, "Readonly").await;
    let other = scalar_i64(
        &db,
        "SELECT id AS v FROM roles WHERE name NOT IN ('Readonly', 'Administrator') \
         AND deleted_at IS NULL AND status != 'retired' ORDER BY id LIMIT 1",
        vec![],
    )
    .await;
    assign_role_scope_impl(&state, &caller, assign(user, readonly, "tenant", None), false, None)
        .await
        .unwrap();
    assign_role_scope_impl(&state, &caller, assign(user, other, "tenant", None), false, None)
        .await
        .unwrap();
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) AS v FROM admin_change_events \
             WHERE action = 'role_revoked' AND target_user_id = ? AND target_role_id = ?",
            vec![user.into(), readonly.into()],
        )
        .await,
        1
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) AS v FROM user_scope_assignments WHERE user_id = ? AND deleted_at IS NULL",
            vec![user.into()],
        )
        .await,
        1
    );
}

#[test]
fn usernames_are_trimmed_and_restricted() {
    assert_eq!(normalize_username("  j.doe ").unwrap(), "j.doe");
    assert_eq!(normalize_username("élodie-m").unwrap(), "élodie-m");
    assert!(normalize_username("ab").is_err());
    assert!(normalize_username("john doe").is_err());
    assert!(normalize_username("x<script>").is_err());
    assert!(normalize_username(&"a".repeat(65)).is_err());
}

#[tokio::test]
async fn usernames_are_unique_regardless_of_case_and_spaces() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let readonly = role_id(&db, "Readonly").await;
    let input = |name: &str| CreateUserInput {
        username: name.into(),
        identity_mode: "local".into(),
        role_id: readonly,
        personnel_id: None,
        initial_password: Some("ValidPass1".into()),
        force_password_change: None,
    };
    create_user_impl(&state, &caller, input(" New.User "))
        .await
        .expect("created trimmed");
    assert!(create_user_impl(&state, &caller, input("NEW.USER")).await.is_err());
    assert!(create_user_impl(&state, &caller, input("ADMIN")).await.is_err());
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) AS v FROM user_accounts WHERE username = 'New.User'",
            vec![],
        )
        .await,
        1
    );
}

#[tokio::test]
async fn refusals_of_non_administrators_go_to_the_security_journal() {
    let db = crate::db::test_db::seeded().await;
    let outsider = insert_user(&db, "no_admin_rights").await;
    let result: AppResult<()> = Err(AppError::PermissionDenied(crate::rbac::permissions::ADM_USERS.into()));
    crate::commands::admin_governance::record_blocked_admin_action(
        &db,
        outsider,
        "user_deactivated",
        None,
        None,
        &result,
    )
    .await;
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) AS v FROM admin_change_events WHERE actor_id = ?",
            vec![outsider.into()],
        )
        .await,
        0,
        "no ledger row for someone without administration rights"
    );
}

// ── Anti-escalation, session revocation, forced password change ────────────────────────────

/// A custom role holding exactly `permissions`, and an account holding it plant-wide.
async fn account_with_permissions(db: &DatabaseConnection, username: &str, permissions: &[&str]) -> (i64, i64) {
    let now = chrono::Utc::now().to_rfc3339();
    let role_name = format!("{username}-role");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
         VALUES (?, ?, 'custom', 'active', 0, ?, ?, 1)",
        [
            format!("{role_name}-sync").into(),
            role_name.clone().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .unwrap();
    let role = role_id(db, &role_name).await;
    for p in permissions {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) SELECT ?, id, ? FROM permissions WHERE name = ?",
            [role.into(), now.clone().into(), (*p).into()],
        ))
        .await
        .unwrap();
    }
    let user = insert_user(db, username).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, created_at, updated_at, row_version) \
         VALUES (?, ?, ?, 'tenant', ?, ?, 1)",
        [
            format!("{username}-usa").into(),
            user.into(),
            role.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .unwrap();
    (user, role)
}

fn caller_for(user_id: i64, username: &str) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: i32::try_from(user_id).expect("fits"),
        username: username.into(),
        display_name: None,
        is_admin: false,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    }
}

async fn open_session(state: &AppState, db: &DatabaseConnection, user: i64, name: &str) -> String {
    let session_id = {
        let mut sm = state.session.write().await;
        sm.create_session(caller_for(user, name)).session_db_id.clone()
    };
    crate::auth::session_manager::create_session_record(
        db,
        &session_id,
        i32::try_from(user).unwrap(),
        &(chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339(),
    )
    .await
    .unwrap();
    session_id
}

#[tokio::test]
async fn a_user_administrator_cannot_escalate() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let (ops_id, ops_role) = account_with_permissions(&db, "ops_admin", &[crate::rbac::permissions::ADM_USERS]).await;
    let ops = caller_for(ops_id, "ops_admin");
    let admin = admin_caller(&db).await;
    let admin_role = role_id(&db, "Administrator").await;
    let readonly = role_id(&db, "Readonly").await;
    let other = insert_user(&db, "escalation_target").await;
    let denied = |r: &AppResult<IdPayload>| matches!(r, Err(AppError::PermissionDenied(_)));

    // Administrator roles: plant administrators only.
    let r = assign_role_scope_impl(&state, &ops, assign(other, admin_role, "tenant", None), false, None).await;
    assert!(denied(&r), "{r:?}");
    // No role for oneself, not even one already held.
    let r = assign_role_scope_impl(&state, &ops, assign(ops_id, ops_role, "tenant", None), false, None).await;
    assert!(denied(&r), "{r:?}");
    // A role with permissions the caller lacks.
    let r = assign_role_scope_impl(&state, &ops, assign(other, readonly, "tenant", None), false, None).await;
    assert!(denied(&r), "{r:?}");
    // A role within the caller's own permissions is fine.
    assign_role_scope_impl(&state, &ops, assign(other, ops_role, "tenant", None), false, None)
        .await
        .expect("grant within own permissions");
    // No self emergency grant.
    let r = grant_emergency_elevation_impl(
        &state,
        &ops,
        GrantEmergencyElevationInput {
            user_id: ops_id,
            role_id: admin_role,
            scope_type: "tenant".into(),
            scope_reference: None,
            reason: "please".into(),
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        },
    )
    .await;
    assert!(denied(&r), "{r:?}");

    // Administrator accounts are managed by administrators only.
    let admin_id = i64::from(admin.user_id);
    let reset = admin_set_user_password_impl(
        &state,
        &ops,
        AdminSetUserPasswordInput {
            user_id: admin_id,
            temporary_password: None,
        },
    )
    .await;
    assert!(matches!(reset, Err(AppError::PermissionDenied(_))), "{reset:?}");
    assert!(matches!(
        unlock_user_account_impl(&state, &ops, admin_id).await,
        Err(AppError::PermissionDenied(_))
    ));
    let disable = set_user_active_impl(
        &state,
        &ops,
        SetUserActiveInput {
            user_id: admin_id,
            reason: "takeover".into(),
        },
        false,
    )
    .await;
    assert!(matches!(disable, Err(AppError::PermissionDenied(_))), "{disable:?}");

    // A role editor cannot add a permission they do not hold.
    let (editor_id, _) = account_with_permissions(&db, "role_editor", &[crate::rbac::permissions::ADM_ROLES]).await;
    let editor = caller_for(editor_id, "role_editor");
    let add = update_role_impl(
        &state,
        &editor,
        UpdateRoleInput {
            role_id: ops_role,
            description: None,
            add_permissions: Some(vec![crate::rbac::permissions::ADM_SETTINGS.to_string()]),
            remove_permissions: None,
        },
    )
    .await;
    assert!(matches!(add, Err(AppError::PermissionDenied(_))), "{add:?}");

    // The plant administrator may grant an administrator role, never to themselves.
    assign_role_scope_impl(&state, &admin, assign(other, admin_role, "tenant", None), false, None)
        .await
        .expect("administrator grants an administrator role");
    let own = assign_role_scope_impl(&state, &admin, assign(admin_id, readonly, "tenant", None), false, None).await;
    assert!(denied(&own), "{own:?}");
}

#[tokio::test]
async fn closed_sessions_and_disabled_accounts_stop_at_the_next_command() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let admin = admin_caller(&db).await;
    let worker = insert_user(&db, "session_worker").await;
    let session_id = open_session(&state, &db, worker, "session_worker").await;
    let user = caller_for(worker, "session_worker");
    crate::auth::session_manager::enforce_stored_session(&db, &state.session, &user, Some(&session_id))
        .await
        .expect("open session passes");

    // Disabling the account closes the stored session and ends the in-memory one.
    set_user_active_impl(
        &state,
        &admin,
        SetUserActiveInput {
            user_id: worker,
            reason: "Contract ended".into(),
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
            vec![session_id.clone().into()],
        )
        .await,
        1
    );
    let refused =
        crate::auth::session_manager::enforce_stored_session(&db, &state.session, &user, Some(&session_id)).await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
    assert!(
        state.session.read().await.current.is_none(),
        "the in-memory session ended"
    );
}

#[tokio::test]
async fn a_revoked_session_row_ends_the_live_session() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "revoked_worker").await;
    let session_id = open_session(&state, &db, worker, "revoked_worker").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE app_sessions SET is_revoked = 1 WHERE id = ?",
        [session_id.clone().into()],
    ))
    .await
    .unwrap();
    let refused = crate::auth::session_manager::enforce_stored_session(
        &db,
        &state.session,
        &caller_for(worker, "revoked_worker"),
        Some(&session_id),
    )
    .await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
    assert!(state.session.read().await.current.is_none());
}

#[tokio::test]
async fn a_password_reset_closes_sessions_and_forces_the_change_first() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let admin = admin_caller(&db).await;
    let worker = insert_user(&db, "reset_worker").await;
    let session_id = open_session(&state, &db, worker, "reset_worker").await;
    let result = admin_set_user_password_impl(
        &state,
        &admin,
        AdminSetUserPasswordInput {
            user_id: worker,
            temporary_password: None,
        },
    )
    .await
    .expect("reset");
    assert!(result.temporary_password.len() >= 16);
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
            vec![session_id.into()],
        )
        .await,
        1,
        "the sessions opened with the old password are closed"
    );

    // A fresh session of an account that must change its password is refused everything.
    let fresh = open_session(&state, &db, worker, "reset_worker").await;
    let refused = crate::auth::session_manager::enforce_stored_session(
        &db,
        &state.session,
        &caller_for(worker, "reset_worker"),
        Some(&fresh),
    )
    .await;
    match refused {
        Err(AppError::Auth(message)) => {
            assert!(message.starts_with(crate::auth::session_manager::PASSWORD_CHANGE_REQUIRED));
        }
        other => panic!("expected a password-change refusal, got {other:?}"),
    }
    assert!(
        state
            .session
            .read()
            .await
            .current
            .as_ref()
            .is_some_and(|s| s.user.force_password_change),
        "the shell is told to show the change-password screen"
    );
}

#[tokio::test]
async fn emergency_access_respects_the_configured_maximum() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let admin = admin_caller(&db).await;
    let user = insert_user(&db, "long_emergency").await;
    let readonly = role_id(&db, "Readonly").await;
    let too_long = grant_emergency_elevation_impl(
        &state,
        &admin,
        GrantEmergencyElevationInput {
            user_id: user,
            role_id: readonly,
            scope_type: "tenant".into(),
            scope_reference: None,
            reason: "Weekend shutdown".into(),
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(9)).to_rfc3339(),
        },
    )
    .await;
    assert!(matches!(too_long, Err(AppError::ValidationFailed(_))), "{too_long:?}");
}

#[tokio::test]
async fn admin_set_passwords_follow_the_plant_policy_too() {
    let db = crate::db::test_db::seeded().await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE rbac_settings SET value = '14' WHERE key = 'password_min_length'".to_string(),
    ))
    .await
    .unwrap();
    assert!(
        validate_admin_set_password(&db, "ValidPass1").await.is_err(),
        "the policy needs 14"
    );
    assert!(validate_admin_set_password(&db, "ValidPassword1234").await.is_ok());
    assert!(validate_admin_set_password(&db, "short").await.is_err());
}
