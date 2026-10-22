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

/// A plant-wide assignment names no tenant or this plant's licence tenant, never another text: it
/// travels to the other PCs unchanged.
#[tokio::test]
async fn plant_wide_assignment_names_no_tenant_or_the_licence_tenant() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let user = insert_user(&db, "tenant_scope_user").await;
    let readonly = role_id(&db, "Readonly").await;

    for reference in ["tenant-other", "plant-01"] {
        let refused = assign_role_scope_impl(
            &state,
            &caller,
            assign(user, readonly, "tenant", Some(reference)),
            false,
            None,
        )
        .await;
        assert!(
            matches!(refused, Err(AppError::ValidationFailed(_))),
            "{reference} without an activated licence"
        );
    }
    crate::commands::product_license::record_activation_claim_for_tests(&db, "plant-01", "fp-scope", "token-scope")
        .await
        .expect("activation");
    let refused = assign_role_scope_impl(
        &state,
        &caller,
        assign(user, readonly, "tenant", Some("tenant-other")),
        false,
        None,
    )
    .await;
    assert!(matches!(refused, Err(AppError::ValidationFailed(_))), "another tenant");
    assign_role_scope_impl(
        &state,
        &caller,
        assign(user, readonly, "tenant", Some("plant-01")),
        false,
        None,
    )
    .await
    .expect("the licence tenant");
    validate_tenant_reference(&db, "tenant", None).await.expect("no tenant");
    validate_tenant_reference(&db, "tenant", Some(" "))
        .await
        .expect("blank");
    validate_tenant_reference(&db, "site", Some("anything"))
        .await
        .expect("not a plant-wide scope");
}

/// Every plant-wide row is one scope: a role given plant-wide from the screens (no tenant named)
/// replaces a plant-wide role recorded with the licence tenant (first administrator, user
/// creation), and a blank reference is stored as none.
#[tokio::test]
async fn a_plant_wide_role_replaces_the_plant_wide_role_naming_the_tenant() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let user = insert_user(&db, "plant_wide_replace").await;
    let readonly = role_id(&db, "Readonly").await;
    let supervisor = role_id(&db, "Supervisor").await;
    crate::commands::product_license::record_activation_claim_for_tests(&db, "plant-01", "fp-replace", "token-replace")
        .await
        .expect("activation");
    assign_role_scope_impl(
        &state,
        &caller,
        assign(user, readonly, "tenant", Some("plant-01")),
        false,
        None,
    )
    .await
    .expect("plant-wide role naming the tenant");
    assign_role_scope_impl(
        &state,
        &caller,
        assign(user, supervisor, "tenant", Some(" ")),
        false,
        None,
    )
    .await
    .expect("plant-wide role from the screens");

    let live = |sql: &'static str| {
        let db = db.clone();
        async move { scalar_i64(&db, sql, vec![user.into()]).await }
    };
    assert_eq!(
        live("SELECT COUNT(*) AS v FROM user_scope_assignments WHERE user_id = ? AND scope_type = 'tenant' AND deleted_at IS NULL").await,
        1,
        "one plant-wide role"
    );
    assert_eq!(
        live("SELECT role_id AS v FROM user_scope_assignments WHERE user_id = ? AND scope_type = 'tenant' AND deleted_at IS NULL").await,
        supervisor
    );
    assert_eq!(
        live("SELECT COUNT(*) AS v FROM user_scope_assignments WHERE user_id = ? AND scope_type = 'tenant' AND deleted_at IS NULL AND scope_reference IS NULL").await,
        1,
        "a blank reference is stored as none"
    );
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
    // The seeded `admin` is also the bootstrap account (`is_admin`), which administers the plant
    // by itself; here its Administrator assignment is the only way anyone administers. Its password
    // is chosen: a built-in account whose password was never changed is not counted as an
    // administrator at all (`auth::last_admin`).
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET is_admin = 0, force_password_change = 0, password_changed_at = '2026-10-19T08:00:00Z' WHERE username = 'admin'"
            .to_string(),
    ))
    .await
    .unwrap();

    // Withdrawing the only administrator assignment is refused and rolled back.
    let revoked = revoke_assignment_impl(&state, &caller, only, None, false).await;
    assert!(
        matches!(&revoked, Err(AppError::OrgValidationFailed(issues))
            if issues.iter().any(|i| i.code == crate::auth::last_admin::LAST_ADMINISTRATOR_REQUIRED)),
        "{revoked:?}"
    );
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
        scope_node_id: None,
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
        Err(AppError::PasswordChangeRequired(message)) => {
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

// ── Security review: expiry, permanent rights only, stronger accounts, stale sessions ────────

/// A raw assignment row (emergency grants, validity windows) for the checks below.
#[allow(clippy::too_many_arguments)]
async fn raw_assignment(
    db: &DatabaseConnection,
    tag: &str,
    user: i64,
    role: i64,
    scope_type: &str,
    scope_reference: Option<&str>,
    emergency_expires_at: Option<String>,
    valid_to: Option<String>,
) {
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments \
         (entity_sync_id, user_id, role_id, scope_type, scope_reference, is_emergency, emergency_reason, \
          emergency_expires_at, valid_to, created_at, updated_at, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            format!("{tag}-usa").into(),
            user.into(),
            role.into(),
            scope_type.into(),
            scope_reference.map(str::to_string).into(),
            i32::from(emergency_expires_at.is_some()).into(),
            emergency_expires_at.as_ref().map(|_| "test grant".to_string()).into(),
            emergency_expires_at.into(),
            valid_to.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .unwrap();
}

async fn clear_assignments(db: &DatabaseConnection, user: i64) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM user_scope_assignments WHERE user_id = ?",
        [user.into()],
    ))
    .await
    .unwrap();
}

fn stamp(offset: chrono::Duration) -> String {
    (chrono::Utc::now() + offset).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[tokio::test]
async fn an_expired_emergency_grant_gives_nothing() {
    use crate::auth::rbac::{check_permission, check_permission_cached, get_user_permissions, PermissionScope};
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let wanted = crate::rbac::permissions::ADM_SETTINGS;
    let (_, settings_role) = account_with_permissions(&db, "settings_source", &[wanted]).await;
    let holder = insert_user(&db, "emergency_holder").await;
    let uid = i32::try_from(holder).unwrap();

    raw_assignment(
        &db,
        "expired-grant",
        holder,
        settings_role,
        "tenant",
        None,
        Some(stamp(-chrono::Duration::minutes(5))),
        None,
    )
    .await;
    assert!(!check_permission(&db, uid, wanted, &PermissionScope::Global)
        .await
        .unwrap());
    assert!(
        !check_permission_cached(&db, &state.permission_cache, uid, wanted, &PermissionScope::Global)
            .await
            .unwrap()
    );
    assert!(!get_user_permissions(&db, uid)
        .await
        .unwrap()
        .iter()
        .any(|p| p.name == wanted));
    assert!(!resolver::all_scope_permissions(&db, holder)
        .await
        .unwrap()
        .contains(wanted));

    // A permanent assignment whose validity ended gives nothing either.
    clear_assignments(&db, holder).await;
    raw_assignment(
        &db,
        "ended-window",
        holder,
        settings_role,
        "tenant",
        None,
        None,
        Some(stamp(-chrono::Duration::minutes(5))),
    )
    .await;
    assert!(!check_permission(&db, uid, wanted, &PermissionScope::Global)
        .await
        .unwrap());

    // A running grant gives it until its expiry, which bounds the cached set.
    clear_assignments(&db, holder).await;
    let expiry = stamp(chrono::Duration::minutes(30));
    raw_assignment(
        &db,
        "running-grant",
        holder,
        settings_role,
        "tenant",
        None,
        Some(expiry.clone()),
        None,
    )
    .await;
    state.permission_cache.write().await.invalidate_user(holder);
    assert!(
        check_permission_cached(&db, &state.permission_cache, uid, wanted, &PermissionScope::Global)
            .await
            .unwrap()
    );
    assert_eq!(
        resolver::next_assignment_change(&db, holder).await.unwrap(),
        Some(expiry),
        "the cache entry ends at the grant's expiry"
    );
}

#[tokio::test]
async fn only_a_permanent_plant_wide_administrator_assignment_elevates() {
    use crate::auth::rbac::user_has_elevated_admin_access;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let admin_role = role_id(&db, "Administrator").await;
    let other = insert_user(&db, "elevation_target").await;

    let emergency = insert_user(&db, "emergency_admin").await;
    raw_assignment(
        &db,
        "emergency-admin",
        emergency,
        admin_role,
        "tenant",
        None,
        Some(stamp(chrono::Duration::hours(1))),
        None,
    )
    .await;
    let ended = insert_user(&db, "ended_admin").await;
    raw_assignment(
        &db,
        "ended-admin",
        ended,
        admin_role,
        "tenant",
        None,
        None,
        Some(stamp(-chrono::Duration::hours(1))),
    )
    .await;
    let site = insert_user(&db, "site_admin").await;
    raw_assignment(&db, "site-admin", site, admin_role, "site", Some("1"), None, None).await;
    let plant = insert_user(&db, "plant_admin").await;
    raw_assignment(&db, "plant-admin", plant, admin_role, "tenant", None, None, None).await;

    for (id, name) in [(emergency, "emergency"), (ended, "ended"), (site, "site-scoped")] {
        let uid = i32::try_from(id).unwrap();
        assert!(
            !user_has_elevated_admin_access(&db, uid).await.unwrap(),
            "{name} admin elevates"
        );
        assert!(!is_plant_admin(&db, id).await.unwrap(), "{name} admin is a plant admin");
    }
    assert!(user_has_elevated_admin_access(&db, i32::try_from(plant).unwrap())
        .await
        .unwrap());
    let bootstrap = admin_caller(&db).await;
    assert!(
        is_plant_admin(&db, i64::from(bootstrap.user_id)).await.unwrap(),
        "the bootstrap admin keeps working"
    );

    // An emergency administrator cannot hand out an administrator role.
    let r = assign_role_scope_impl(
        &state,
        &caller_for(emergency, "emergency_admin"),
        assign(other, admin_role, "tenant", None),
        false,
        None,
    )
    .await;
    assert!(matches!(r, Err(AppError::PermissionDenied(_))), "{r:?}");
}

#[tokio::test]
async fn an_emergency_grant_never_lets_its_holder_grant_what_it_brings() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let (_, settings_role) =
        account_with_permissions(&db, "grant_source", &[crate::rbac::permissions::ADM_SETTINGS]).await;
    let (ops_id, _) = account_with_permissions(&db, "temp_ops", &[crate::rbac::permissions::ADM_USERS]).await;
    raw_assignment(
        &db,
        "temp-ops-grant",
        ops_id,
        settings_role,
        "tenant",
        None,
        Some(stamp(chrono::Duration::hours(1))),
        None,
    )
    .await;
    let other = insert_user(&db, "temp_target").await;
    let r = assign_role_scope_impl(
        &state,
        &caller_for(ops_id, "temp_ops"),
        assign(other, settings_role, "tenant", None),
        false,
        None,
    )
    .await;
    assert!(matches!(r, Err(AppError::PermissionDenied(_))), "{r:?}");
}

#[tokio::test]
async fn a_user_manager_cannot_take_over_a_stronger_account() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let (ops_id, _) = account_with_permissions(&db, "user_manager", &[crate::rbac::permissions::ADM_USERS]).await;
    let ops = caller_for(ops_id, "user_manager");
    let (strong, _) = account_with_permissions(
        &db,
        "strong_account",
        &[
            crate::rbac::permissions::ADM_USERS,
            crate::rbac::permissions::ADM_SETTINGS,
        ],
    )
    .await;
    let (weak, _) = account_with_permissions(&db, "weak_account", &[crate::rbac::permissions::ADM_USERS]).await;
    let denied = |r: &AppResult<()>| matches!(r, Err(AppError::PermissionDenied(_)));

    let reset = admin_set_user_password_impl(
        &state,
        &ops,
        AdminSetUserPasswordInput {
            user_id: strong,
            temporary_password: None,
        },
    )
    .await;
    assert!(matches!(reset, Err(AppError::PermissionDenied(_))), "{reset:?}");
    assert!(denied(&unlock_user_account_impl(&state, &ops, strong).await));
    let disable = set_user_active_impl(
        &state,
        &ops,
        SetUserActiveInput {
            user_id: strong,
            reason: "Takeover attempt".into(),
        },
        false,
    )
    .await;
    assert!(denied(&disable), "{disable:?}");
    let edit = update_user_impl(
        &state,
        &ops,
        UpdateUserInput {
            user_id: strong,
            username: None,
            display_name: Some("Taken over".into()),
            email: None,
            phone: None,
            personnel_id: None,
            force_password_change: None,
            is_active: None,
        },
    )
    .await;
    assert!(denied(&edit), "{edit:?}");
    // Ending a session of the stronger account (revoke_session) uses the same rule.
    assert!(denied(&ensure_may_manage_account(&db, &ops, strong).await));

    // An account within the manager's own permissions can be managed.
    admin_set_user_password_impl(
        &state,
        &ops,
        AdminSetUserPasswordInput {
            user_id: weak,
            temporary_password: None,
        },
    )
    .await
    .expect("reset of an account the manager covers");
}

#[tokio::test]
async fn sessions_older_than_the_password_end_even_when_it_changed_on_another_device() {
    use crate::auth::session_manager::enforce_stored_session;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "roaming_worker").await;
    let user = caller_for(worker, "roaming_worker");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_changed_at = '2026-01-05T08:00:00Z' WHERE id = ?",
        [worker.into()],
    ))
    .await
    .unwrap();
    let session_id = open_session(&state, &db, worker, "roaming_worker").await;
    enforce_stored_session(&db, &state.session, &user, Some(&session_id))
        .await
        .expect("open session passes");

    // The same date received again with another precision and offset does not end it.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_changed_at = '2026-01-05T09:00:00.000+01:00' WHERE id = ?",
        [worker.into()],
    ))
    .await
    .unwrap();
    enforce_stored_session(&db, &state.session, &user, Some(&session_id))
        .await
        .expect("the same date");

    // An administrator reset on another device arrives by sync, ten minutes after the session
    // started: new password, change required.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE app_sessions SET created_at = ? WHERE id = ?",
        [
            (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339().into(),
            session_id.clone().into(),
        ],
    ))
    .await
    .unwrap();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_changed_at = ?, force_password_change = 1 WHERE id = ?",
        [chrono::Utc::now().to_rfc3339().into(), worker.into()],
    ))
    .await
    .unwrap();
    let refused = enforce_stored_session(&db, &state.session, &user, Some(&session_id)).await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
            vec![session_id.clone().into()],
        )
        .await,
        1,
        "the stale session row is closed"
    );
    assert!(state.session.read().await.current.is_none());

    // The forced change itself refuses an ended session.
    {
        let mut sm = state.session.write().await;
        sm.create_session(caller_for(worker, "roaming_worker"));
        sm.current.as_mut().unwrap().session_db_id = session_id.clone();
    }
    let forced = crate::commands::auth::force_change_password_impl(
        &state,
        crate::commands::auth::ForceChangePasswordRequest {
            new_password: "NewPassword12".into(),
        },
    )
    .await;
    assert!(matches!(forced, Err(AppError::Auth(_))), "{forced:?}");

    // No stored row: no session (fail closed).
    let ghost = {
        let mut sm = state.session.write().await;
        sm.create_session(caller_for(worker, "roaming_worker"))
            .session_db_id
            .clone()
    };
    let refused = enforce_stored_session(&db, &state.session, &user, Some(&ghost)).await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
}

#[tokio::test]
async fn the_forced_change_follows_the_policy_and_leaves_one_live_session() {
    use crate::auth::session_manager::enforce_stored_session;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let admin = admin_caller(&db).await;
    let worker = insert_user(&db, "forced_worker").await;
    let user = caller_for(worker, "forced_worker");
    admin_set_user_password_impl(
        &state,
        &admin,
        AdminSetUserPasswordInput {
            user_id: worker,
            temporary_password: None,
        },
    )
    .await
    .expect("reset");
    let session_id = open_session(&state, &db, worker, "forced_worker").await;
    let refused = enforce_stored_session(&db, &state.session, &user, Some(&session_id)).await;
    assert!(
        matches!(refused, Err(AppError::PasswordChangeRequired(_))),
        "{refused:?}"
    );

    let change = |p: &str| crate::commands::auth::ForceChangePasswordRequest { new_password: p.into() };
    let weak = crate::commands::auth::force_change_password_impl(&state, change("short")).await;
    assert!(matches!(weak, Err(AppError::ValidationFailed(_))), "{weak:?}");
    crate::commands::auth::force_change_password_impl(&state, change("NewPassword12"))
        .await
        .expect("change");

    let current = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .unwrap()
        .session_db_id
        .clone();
    assert_ne!(current, session_id, "the session continues on a new stored row");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
            vec![session_id.into()],
        )
        .await,
        1
    );
    enforce_stored_session(&db, &state.session, &user, Some(&current))
        .await
        .expect("the changed session works");
    assert!(
        !state
            .session
            .read()
            .await
            .current
            .as_ref()
            .unwrap()
            .user
            .force_password_change
    );

    // Once the change is no longer required (made elsewhere), the screen is left.
    state
        .session
        .write()
        .await
        .current
        .as_mut()
        .unwrap()
        .user
        .force_password_change = true;
    let not_required = crate::commands::auth::force_change_password_impl(&state, change("OtherPassword12")).await;
    assert!(
        matches!(not_required, Err(AppError::ValidationFailed(_))),
        "{not_required:?}"
    );
    assert!(
        !state
            .session
            .read()
            .await
            .current
            .as_ref()
            .unwrap()
            .user
            .force_password_change
    );
}

#[tokio::test]
async fn ones_own_password_change_follows_the_policy_and_closes_other_sessions() {
    use crate::auth::session_manager::enforce_stored_session;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "own_change").await;
    let hash = crate::auth::password::hash_password("OldPassword12").unwrap();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_hash = ? WHERE id = ?",
        [hash.into(), worker.into()],
    ))
    .await
    .unwrap();
    let other_device = "other-device-session";
    crate::auth::session_manager::create_session_record(
        &db,
        other_device,
        i32::try_from(worker).unwrap(),
        &(chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339(),
    )
    .await
    .unwrap();
    let session_id = open_session(&state, &db, worker, "own_change").await;
    let payload = |new: &str| crate::commands::profile::ChangePasswordPayload {
        current_password: "OldPassword12".into(),
        new_password: new.into(),
    };

    let weak = crate::commands::profile::change_password_impl(&state, payload("abcdefgh")).await;
    assert!(matches!(weak, Err(AppError::ValidationFailed(_))), "{weak:?}");
    crate::commands::profile::change_password_impl(&state, payload("NewPassword12"))
        .await
        .expect("change");

    for closed in [other_device.to_string(), session_id] {
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
                vec![closed.into()],
            )
            .await,
            1
        );
    }
    let current = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .unwrap()
        .session_db_id
        .clone();
    enforce_stored_session(&db, &state.session, &caller_for(worker, "own_change"), Some(&current))
        .await
        .expect("this session continues");
}

// ── Security re-review: every path uses the shared rule; atomic session writes ──────────────

#[tokio::test]
async fn an_expired_grant_cannot_sign_a_parts_override() {
    let db = crate::db::test_db::seeded().await;
    let supervisor = role_id(&db, "Supervisor").await;
    let allowed = vec!["Supervisor".to_string()];
    let user = insert_user(&db, "override_signer").await;
    raw_assignment(
        &db,
        "override-expired",
        user,
        supervisor,
        "tenant",
        None,
        Some(stamp(-chrono::Duration::minutes(1))),
        None,
    )
    .await;
    assert!(!crate::wo::closeout::has_supervisor_override_role(&db, user, &allowed)
        .await
        .unwrap());
    clear_assignments(&db, user).await;
    raw_assignment(
        &db,
        "override-ended",
        user,
        supervisor,
        "tenant",
        None,
        None,
        Some(stamp(-chrono::Duration::minutes(1))),
    )
    .await;
    assert!(!crate::wo::closeout::has_supervisor_override_role(&db, user, &allowed)
        .await
        .unwrap());
    clear_assignments(&db, user).await;
    raw_assignment(&db, "override-live", user, supervisor, "tenant", None, None, None).await;
    assert!(crate::wo::closeout::has_supervisor_override_role(&db, user, &allowed)
        .await
        .unwrap());
}

#[tokio::test]
async fn an_approval_limit_ends_with_its_grant_not_at_midnight() {
    use crate::inventory::approvals::{ensure_can_approve, upsert_limit, DOC_REQUISITION};
    let db = crate::db::test_db::seeded().await;
    let (_, approver_role) = account_with_permissions(&db, "limit_source", &[]).await;
    upsert_limit(
        &db,
        crate::inventory::domain::ProcurementApprovalLimitInput {
            role_id: approver_role,
            document_type: DOC_REQUISITION.into(),
            max_amount: None,
            is_active: Some(true),
            actor_id: None,
        },
    )
    .await
    .expect("limit");
    let author = insert_user(&db, "limit_author").await;
    let approver = insert_user(&db, "limit_approver").await;
    let approve = || ensure_can_approve(&db, DOC_REQUISITION, 1, Some(author), Some(approver));

    // An emergency grant that lapsed a minute ago (same day) no longer carries the limit.
    raw_assignment(
        &db,
        "limit-lapsed",
        approver,
        approver_role,
        "tenant",
        None,
        Some(stamp(-chrono::Duration::minutes(1))),
        None,
    )
    .await;
    assert!(approve().await.is_err(), "a lapsed grant keeps no approval limit");
    clear_assignments(&db, approver).await;
    // Nor does an assignment whose validity ended a minute ago.
    raw_assignment(
        &db,
        "limit-ended",
        approver,
        approver_role,
        "tenant",
        None,
        None,
        Some(stamp(-chrono::Duration::minutes(1))),
    )
    .await;
    assert!(approve().await.is_err(), "an ended assignment keeps no approval limit");
    clear_assignments(&db, approver).await;
    raw_assignment(
        &db,
        "limit-running",
        approver,
        approver_role,
        "tenant",
        None,
        Some(stamp(chrono::Duration::hours(1))),
        None,
    )
    .await;
    approve().await.expect("a running grant carries the limit");
}

#[tokio::test]
async fn the_last_admin_guard_counts_like_plant_administration() {
    let db = crate::db::test_db::seeded().await;
    let admin_role = role_id(&db, "Administrator").await;
    let base = active_admin_holders(&db).await.unwrap().active;

    let site = insert_user(&db, "guard_site_admin").await;
    raw_assignment(&db, "guard-site", site, admin_role, "site", Some("1"), None, None).await;
    let emergency = insert_user(&db, "guard_emergency_admin").await;
    raw_assignment(
        &db,
        "guard-emergency",
        emergency,
        admin_role,
        "tenant",
        None,
        Some(stamp(chrono::Duration::hours(1))),
        None,
    )
    .await;
    let ended = insert_user(&db, "guard_ended_admin").await;
    raw_assignment(
        &db,
        "guard-ended",
        ended,
        admin_role,
        "tenant",
        None,
        None,
        Some(stamp(-chrono::Duration::hours(1))),
    )
    .await;
    assert_eq!(
        active_admin_holders(&db).await.unwrap().active,
        base,
        "site-scoped, emergency and ended administrators cannot administer the plant"
    );

    let plant = insert_user(&db, "guard_plant_admin").await;
    raw_assignment(&db, "guard-plant", plant, admin_role, "tenant", None, None, None).await;
    assert_eq!(active_admin_holders(&db).await.unwrap().active, base + 1);
    for id in [site, emergency, ended, plant] {
        assert_eq!(
            is_plant_admin(&db, id).await.unwrap(),
            id == plant,
            "the guard and the plant-administrator rule agree"
        );
    }
}

async fn block_session_rows(db: &DatabaseConnection, blocked: bool) {
    let sql = if blocked {
        "CREATE TRIGGER block_session_rows BEFORE INSERT ON app_sessions \
         BEGIN SELECT RAISE(ABORT, 'disk full'); END"
    } else {
        "DROP TRIGGER block_session_rows"
    };
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_password_change_and_its_session_rotation_commit_together() {
    use crate::auth::session_manager::change_password_and_rotate;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "atomic_change").await;
    let uid = i32::try_from(worker).unwrap();
    let session_id = open_session(&state, &db, worker, "atomic_change").await;
    let hash = || async {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT password_hash FROM user_accounts WHERE id = ?",
            [worker.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "password_hash")
        .unwrap()
    };
    let revoked = |id: String| {
        let db = db.clone();
        async move {
            scalar_i64(
                &db,
                "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
                vec![id.into()],
            )
            .await
        }
    };

    // The new session row cannot be written: nothing changes, not even the password.
    block_session_rows(&db, true).await;
    let failed = change_password_and_rotate(&db, &state.session, uid, "new-hash", false).await;
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(hash().await, "x", "no password written without its rotation");
    assert_eq!(revoked(session_id.clone()).await, 0, "the current session stays open");
    assert_eq!(
        state.session.read().await.current.as_ref().unwrap().session_db_id,
        session_id
    );

    block_session_rows(&db, false).await;
    change_password_and_rotate(&db, &state.session, uid, "new-hash", false)
        .await
        .expect("change");
    assert_eq!(hash().await, "new-hash");
    assert_eq!(revoked(session_id).await, 1);
    let current = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .unwrap()
        .session_db_id
        .clone();
    assert_eq!(revoked(current).await, 0, "only the rotated session is alive");
}

#[tokio::test]
async fn a_sign_in_whose_session_row_fails_leaves_no_session_in_memory() {
    use crate::auth::session_manager::{store_session_or_clear, SessionManager};
    let db = crate::db::test_db::seeded().await;
    let worker = insert_user(&db, "row_less_login").await;
    let uid = i32::try_from(worker).unwrap();
    let mut manager = SessionManager::new();
    let (session_id, expires) = {
        let s = manager.create_session(caller_for(worker, "row_less_login"));
        (s.session_db_id.clone(), s.expires_at.to_rfc3339())
    };
    block_session_rows(&db, true).await;
    let failed = store_session_or_clear(&db, &mut manager, uid, &session_id, &expires).await;
    assert!(failed.is_err());
    assert!(manager.current.is_none(), "no row-less session in memory");

    block_session_rows(&db, false).await;
    let (session_id, expires) = {
        let s = manager.create_session(caller_for(worker, "row_less_login"));
        (s.session_db_id.clone(), s.expires_at.to_rfc3339())
    };
    store_session_or_clear(&db, &mut manager, uid, &session_id, &expires)
        .await
        .expect("stored");
    assert!(manager.current.is_some());
}

#[tokio::test]
async fn a_user_created_on_a_site_gets_only_that_site_and_administrators_stay_plant_wide() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = admin_caller(&db).await;
    let readonly = role_id(&db, "Readonly").await;
    let administrator = role_id(&db, "Administrator").await;
    // A site of its own (the seeded test database has no org tree).
    for sql in [
        "INSERT INTO org_structure_models (entity_sync_id, version_number, status, description, created_at, updated_at)
         VALUES ('scoped-user-structure', 1, 'active', 'test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO org_node_types (entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work,
            can_carry_cost_center, can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at)
         SELECT 'scoped-user-type', id, 'SITE', 'Site', 1, 1, 1, 1, 1, 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
           FROM org_structure_models WHERE entity_sync_id = 'scoped-user-structure'",
        "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
            created_at, updated_at, row_version, structure_model_id)
         SELECT 'scoped-user-site', 'SITE-SU', 'Site SU', t.id, NULL, '/', 0, 'active',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, t.structure_model_id
           FROM org_node_types t WHERE t.entity_sync_id = 'scoped-user-type'",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql);
    }
    let site: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE entity_sync_id = 'scoped-user-site'".to_string(),
        ))
        .await
        .expect("query")
        .expect("the site")
        .try_get("", "id")
        .expect("id");
    let input = |name: &str, role: i64, node: Option<i64>| CreateUserInput {
        username: name.into(),
        identity_mode: "local".into(),
        role_id: role,
        scope_node_id: node,
        personnel_id: None,
        initial_password: Some("ValidPass1".into()),
        force_password_change: None,
    };

    // On a site: one assignment on that node, no plant-wide one added.
    let created = create_user_impl(&state, &caller, input("site.tech", readonly, Some(site)))
        .await
        .expect("site user");
    let scopes = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT scope_type, scope_reference FROM user_scope_assignments WHERE user_id = ? AND deleted_at IS NULL",
            [created.id.into()],
        ))
        .await
        .expect("assignments")
        .iter()
        .map(|r| {
            (
                r.try_get::<String>("", "scope_type").expect("type"),
                r.try_get::<Option<String>>("", "scope_reference").expect("reference"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(scopes, vec![("org_node".to_string(), Some(site.to_string()))]);

    // A deleted or unknown node is refused; an administrator role on a site is refused.
    assert!(
        create_user_impl(&state, &caller, input("ghost.tech", readonly, Some(999_999)))
            .await
            .is_err()
    );
    let admin_on_site = create_user_impl(&state, &caller, input("site.admin", administrator, Some(site)))
        .await
        .expect_err("administrator on a site");
    assert!(
        format!("{admin_on_site:?}").contains("SCOPE_ADMIN_ROLE_PLANT_WIDE"),
        "{admin_on_site:?}"
    );

    // Emergency access follows the same rule.
    let in_one_hour = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let emergency = grant_emergency_elevation_impl(
        &state,
        &caller,
        GrantEmergencyElevationInput {
            user_id: created.id,
            role_id: administrator,
            scope_type: "org_node".into(),
            scope_reference: Some(site.to_string()),
            reason: "Line stop".into(),
            expires_at: in_one_hour,
        },
    )
    .await
    .expect_err("emergency administrator on a site");
    assert!(
        format!("{emergency:?}").contains("SCOPE_ADMIN_ROLE_PLANT_WIDE"),
        "{emergency:?}"
    );
}

/// Audit rows of a session-check event for a session (`action_code`, `details_json`).
async fn session_audit(db: &DatabaseConnection, action: &str, session_id: &str) -> Vec<String> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT details_json FROM audit_events WHERE action_code = ? AND target_id = ? ORDER BY id",
        [action.into(), session_id.into()],
    ))
    .await
    .expect("audit rows")
    .iter()
    .map(|r| {
        r.try_get::<Option<String>>("", "details_json")
            .ok()
            .flatten()
            .unwrap_or_default()
    })
    .collect()
}

/// F1 (owner test of 0.9.1): a password date later than this device's clock (written while a
/// clock ran ahead) closed every new session at once, so signing in again never helped. It is now
/// a clock anomaly, recorded once; a real change after sign-in still ends the session.
#[tokio::test]
async fn a_password_date_ahead_of_the_clock_keeps_new_sessions_and_a_real_change_still_ends_them() {
    use crate::auth::session_manager::enforce_stored_session;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "clock_worker").await;
    let user = caller_for(worker, "clock_worker");
    let ahead = (chrono::Utc::now() + chrono::Duration::days(3)).to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_changed_at = ? WHERE id = ?",
        [ahead.into(), worker.into()],
    ))
    .await
    .unwrap();

    let session_id = open_session(&state, &db, worker, "clock_worker").await;
    for _ in 0..3 {
        enforce_stored_session(&db, &state.session, &user, Some(&session_id))
            .await
            .expect("a fresh session survives a password date ahead of the clock");
    }
    let anomalies = session_audit(&db, crate::audit::event_type::SESSION_CLOCK_ANOMALY, &session_id).await;
    assert_eq!(anomalies.len(), 1, "recorded once per session: {anomalies:?}");

    // A real change arrives (on another device, by sync): the stored value changes.
    let changed = (chrono::Utc::now() + chrono::Duration::days(3) + chrono::Duration::hours(1)).to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_changed_at = ? WHERE id = ?",
        [changed.into(), worker.into()],
    ))
    .await
    .unwrap();
    let refused = enforce_stored_session(&db, &state.session, &user, Some(&session_id)).await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
    assert!(state.session.read().await.current.is_none());
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
            vec![session_id.clone().into()],
        )
        .await,
        1
    );
    let closed = session_audit(&db, crate::audit::event_type::SESSION_CLOSED, &session_id).await;
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert!(closed[0].contains("password_changed"), "{closed:?}");
}

/// A change made soon after sign-in (within the clock tolerance of the session start) ends the
/// session once the session has read the account's date: the value changed, whatever the clocks.
#[tokio::test]
async fn a_password_change_soon_after_sign_in_ends_the_session() {
    use crate::auth::session_manager::enforce_stored_session;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "quick_change").await;
    let user = caller_for(worker, "quick_change");
    let session_id = open_session(&state, &db, worker, "quick_change").await;
    enforce_stored_session(&db, &state.session, &user, Some(&session_id))
        .await
        .expect("open session passes");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET password_changed_at = ? WHERE id = ?",
        [chrono::Utc::now().to_rfc3339().into(), worker.into()],
    ))
    .await
    .unwrap();
    let refused = enforce_stored_session(&db, &state.session, &user, Some(&session_id)).await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
}

/// Every end of a session by the stored-state check is audited with its reason.
#[tokio::test]
async fn a_closed_session_is_audited_with_its_reason() {
    use crate::auth::session_manager::enforce_stored_session;
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let worker = insert_user(&db, "revoked_reason").await;
    let user = caller_for(worker, "revoked_reason");
    let session_id = open_session(&state, &db, worker, "revoked_reason").await;
    crate::auth::session_manager::revoke_user_sessions(&db, worker)
        .await
        .unwrap();
    let refused = enforce_stored_session(&db, &state.session, &user, Some(&session_id)).await;
    assert!(matches!(refused, Err(AppError::Auth(_))), "{refused:?}");
    let closed = session_audit(&db, crate::audit::event_type::SESSION_CLOSED, &session_id).await;
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert!(closed[0].contains("\"revoked\""), "{closed:?}");
}
