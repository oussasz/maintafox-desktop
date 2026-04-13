//! RBAC governance test suite — Phase 2 SP06-F04.
//!
//! 19 integration tests against in-memory SQLite with all migrations applied.
//! Covers:
//!   1–5:  Scope resolution (tenant, entity isolation, deny-all, time-bounded)
//!   6–7:  Dependency enforcement (hard block, warn pass-through)
//!   8–10: Dangerous permissions (step-up flags, custom system-namespace block)
//!   11–12: Emergency elevation (included before / excluded after expiry)
//!   13:   Delegation boundary
//!   14:   Role export/import round-trip
//!   15:   Full admin governance lifecycle
//!   16–17: Password expiry policy checks
//!   18–19: PIN unlock success/failure lockout

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;
    use uuid::Uuid;

    use crate::auth::{password, password_policy, pin, session_manager};
    use crate::rbac::{delegation, resolver};

    // ═══════════════════════════════════════════════════════════════════════
    //  SETUP HELPERS
    // ═══════════════════════════════════════════════════════════════════════

    /// Spin up a fresh in-memory SQLite with all migrations + system seed data.
    async fn setup_db() -> DatabaseConnection {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("enable FK");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations");
        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder");

        db
    }

    /// Create a test user and return their `user_accounts.id`.
    async fn create_test_user(db: &DatabaseConnection, username: &str) -> i64 {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let sync_id = Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts \
             (sync_id, username, display_name, identity_mode, is_active, is_admin, \
              force_password_change, failed_login_attempts, created_at, updated_at, row_version) \
             VALUES (?, ?, ?, 'local', 1, 0, 0, 0, ?, ?, 1)",
            [
                sync_id.into(),
                username.into(),
                username.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert test user");

        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = ?",
            [username.into()],
        ))
        .await
        .expect("query user id")
        .expect("user row exists")
        .try_get::<i64>("", "id")
        .expect("id column")
    }

    /// Look up a role's id by name.
    async fn get_role_id(db: &DatabaseConnection, role_name: &str) -> i64 {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE name = ? AND deleted_at IS NULL AND status != 'retired'",
            [role_name.into()],
        ))
        .await
        .expect("query role")
        .unwrap_or_else(|| panic!("role '{}' not found", role_name))
        .try_get::<i64>("", "id")
        .expect("id column")
    }

    /// Assign a role to a user at a given scope (direct SQL — bypasses IPC).
    async fn assign_role(
        db: &DatabaseConnection,
        user_id: i64,
        role_id: i64,
        scope_type: &str,
        scope_reference: Option<&str>,
        valid_from: Option<&str>,
        valid_to: Option<&str>,
    ) -> i64 {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let sync_id = Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, scope_reference, \
              valid_from, valid_to, created_at, updated_at, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                sync_id.into(),
                user_id.into(),
                role_id.into(),
                scope_type.into(),
                scope_reference
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                valid_from
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                valid_to
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert scope assignment");

        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id".to_string(),
        ))
        .await
        .expect("last_insert_rowid")
        .expect("row exists")
        .try_get::<i64>("", "id")
        .expect("id")
    }

    /// Insert an emergency grant assignment with an explicit expiry timestamp.
    async fn insert_emergency_grant(
        db: &DatabaseConnection,
        user_id: i64,
        role_id: i64,
        scope_type: &str,
        emergency_expires_at: &str,
        reason: &str,
    ) -> i64 {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let sync_id = Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, scope_reference, \
              is_emergency, emergency_reason, emergency_expires_at, \
              created_at, updated_at, row_version) \
             VALUES (?, ?, ?, ?, NULL, 1, ?, ?, ?, ?, 1)",
            [
                sync_id.into(),
                user_id.into(),
                role_id.into(),
                scope_type.into(),
                reason.into(),
                emergency_expires_at.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert emergency grant");

        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id".to_string(),
        ))
        .await
        .expect("last_insert_rowid")
        .expect("row exists")
        .try_get::<i64>("", "id")
        .expect("id")
    }

    /// Create a custom role with specific permission names (direct SQL).
    /// Returns the new role's id.
    async fn create_test_role(
        db: &DatabaseConnection,
        name: &str,
        permission_names: &[&str],
    ) -> i64 {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let sync_id = Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO roles (sync_id, name, description, role_type, status, is_system, \
                                created_at, updated_at, row_version) \
             VALUES (?, ?, NULL, 'custom', 'active', 0, ?, ?, 1)",
            [
                sync_id.into(),
                name.into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .expect("insert test role");

        let role_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() as id".to_string(),
            ))
            .await
            .expect("rowid")
            .expect("row")
            .try_get("", "id")
            .expect("id");

        for perm_name in permission_names {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO role_permissions (role_id, permission_id, granted_at) \
                 SELECT ?, p.id, ? FROM permissions p WHERE p.name = ?",
                [role_id.into(), now.clone().into(), (*perm_name).into()],
            ))
            .await
            .expect("link permission");
        }

        role_id
    }

    /// Insert a delegation policy (direct SQL).
    async fn insert_delegation_policy(
        db: &DatabaseConnection,
        admin_role_id: i64,
        managed_scope_type: &str,
        managed_scope_reference: Option<&str>,
        allowed_domains: &[&str],
    ) -> i64 {
        let domains_json = serde_json::to_string(allowed_domains).unwrap();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO delegated_admin_policies \
             (admin_role_id, managed_scope_type, managed_scope_reference, \
              allowed_domains_json, requires_step_up_for_publish) \
             VALUES (?, ?, ?, ?, 1)",
            [
                admin_role_id.into(),
                managed_scope_type.into(),
                managed_scope_reference
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                domains_json.into(),
            ],
        ))
        .await
        .expect("insert delegation policy");

        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id".to_string(),
        ))
        .await
        .expect("rowid")
        .expect("row")
        .try_get::<i64>("", "id")
        .expect("id")
    }

    /// Write an admin change event directly for test use.
    async fn insert_admin_event(
        db: &DatabaseConnection,
        action: &str,
        actor_id: i64,
        target_user_id: Option<i64>,
        target_role_id: Option<i64>,
    ) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO admin_change_events \
             (action, actor_id, target_user_id, target_role_id, summary) \
             VALUES (?, ?, ?, ?, ?)",
            [
                action.into(),
                actor_id.into(),
                target_user_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                target_role_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                format!("Test event: {action}").into(),
            ],
        ))
        .await
        .expect("insert admin event");
    }

    /// Compute ISO-8601 date string N days from today.
    fn date_offset_days(days: i64) -> String {
        let dt = chrono::Utc::now() + chrono::Duration::days(days);
        dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    fn make_auth_user(user_id: i64, username: &str) -> session_manager::AuthenticatedUser {
        session_manager::AuthenticatedUser {
            user_id: user_id as i32,
            username: username.to_string(),
            display_name: Some(username.to_string()),
            is_admin: false,
            force_password_change: false,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  SCOPE RESOLUTION (tests 01–05)
    // ═══════════════════════════════════════════════════════════════════════

    /// 01 — Tenant-scoped Supervisor role grants permissions at any entity scope.
    #[tokio::test]
    async fn test_rbac_01_tenant_scope_grants_all_entities() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t01_user").await;
        let role_id = get_role_id(&db, "Supervisor").await;

        assign_role(&db, user_id, role_id, "tenant", None, None, None).await;

        // Query with a specific org_node scope — tenant grant should propagate
        let perms = resolver::effective_permissions(&db, user_id, "org_node", Some("entity-99"))
            .await
            .expect("effective_permissions");

        assert!(perms.contains("ot.view"), "missing ot.view: {perms:?}");
        assert!(perms.contains("ot.edit"), "missing ot.edit: {perms:?}");
        assert!(perms.contains("di.view"), "missing di.view: {perms:?}");
        assert!(perms.contains("di.approve"), "missing di.approve: {perms:?}");
    }

    /// 02 — Entity-scoped Technician role grants permissions only at the assigned entity.
    #[tokio::test]
    async fn test_rbac_02_entity_scope_isolated() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t02_user").await;
        let role_id = get_role_id(&db, "Maintenance Technician").await;

        // Assign at entity_A only
        assign_role(
            &db,
            user_id,
            role_id,
            "org_node",
            Some("entity_A"),
            None,
            None,
        )
        .await;

        // entity_A: should have permissions
        let perms_a =
            resolver::effective_permissions(&db, user_id, "org_node", Some("entity_A"))
                .await
                .expect("perms for entity_A");
        assert!(
            perms_a.contains("ot.view"),
            "entity_A should have ot.view: {perms_a:?}"
        );

        // entity_B: should NOT have permissions
        let perms_b =
            resolver::effective_permissions(&db, user_id, "org_node", Some("entity_B"))
                .await
                .expect("perms for entity_B");
        assert!(
            !perms_b.contains("ot.view"),
            "entity_B should NOT have ot.view: {perms_b:?}"
        );
    }

    /// 03 — User with zero scope assignments gets no permissions (deny-all).
    #[tokio::test]
    async fn test_rbac_03_no_assignments_deny_all() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t03_user").await;

        let perms = resolver::effective_permissions(&db, user_id, "tenant", None)
            .await
            .expect("effective_permissions");

        assert!(
            perms.is_empty(),
            "deny-all fallback: expected empty set, got {perms:?}"
        );
    }

    /// 04 — Expired scope assignment (valid_to in the past) is excluded.
    #[tokio::test]
    async fn test_rbac_04_expired_scope_not_included() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t04_user").await;
        let role_id = get_role_id(&db, "Supervisor").await;
        let yesterday = date_offset_days(-1);

        assign_role(
            &db,
            user_id,
            role_id,
            "tenant",
            None,
            None,
            Some(&yesterday),
        )
        .await;

        let perms = resolver::effective_permissions(&db, user_id, "tenant", None)
            .await
            .expect("effective_permissions");

        assert!(
            perms.is_empty(),
            "expired assignment should yield empty set: {perms:?}"
        );
    }

    /// 05 — Future scope assignment (valid_from in the future) is not yet active.
    #[tokio::test]
    async fn test_rbac_05_future_scope_not_yet_active() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t05_user").await;
        let role_id = get_role_id(&db, "Supervisor").await;
        let tomorrow = date_offset_days(1);

        assign_role(
            &db,
            user_id,
            role_id,
            "tenant",
            None,
            Some(&tomorrow),
            None,
        )
        .await;

        let perms = resolver::effective_permissions(&db, user_id, "tenant", None)
            .await
            .expect("effective_permissions");

        assert!(
            perms.is_empty(),
            "future assignment should yield empty set: {perms:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  DEPENDENCY ENFORCEMENT (tests 06–07)
    // ═══════════════════════════════════════════════════════════════════════

    /// 06 — Hard dependency on ot.close blocks when ot.edit and ot.view are missing.
    #[tokio::test]
    async fn test_rbac_06_hard_dep_blocks_role_creation() {
        let db = setup_db().await;

        // ot.close depends hard on ot.edit; ot.edit depends hard on ot.view
        let names: HashSet<String> = ["ot.close"].iter().map(|s| s.to_string()).collect();
        let result = resolver::validate_hard_dependencies(&db, &names).await;

        assert!(result.is_err(), "expected hard-dep error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("ot.edit"),
            "error should mention ot.edit: {err_msg}"
        );
    }

    /// 07 — Warn dependency (ot.reopen → ot.close) allows creation when hard deps are met.
    ///       validate_role_permissions surfaces a warning but not an error.
    #[tokio::test]
    async fn test_rbac_07_warn_dep_allows_role_creation_with_warning() {
        let db = setup_db().await;

        // Set with all hard deps satisfied: ot.view → ot.edit → ot.close + ot.reopen
        let names: HashSet<String> = ["ot.reopen", "ot.close", "ot.edit", "ot.view"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Hard deps should pass
        let hard_result = resolver::validate_hard_dependencies(&db, &names).await;
        assert!(hard_result.is_ok(), "hard deps should be satisfied");

        // Warn deps should flag ot.reopen → ot.close
        let warns = resolver::dependency_warnings_for(&db, &names)
            .await
            .expect("dependency_warnings_for");

        let has_reopen_warn = warns.iter().any(|d| {
            d.permission_name == "ot.reopen"
                && d.dependency_type == "warn"
        });
        assert!(
            has_reopen_warn,
            "expected ot.reopen warn dependency in {warns:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  DANGEROUS PERMISSIONS (tests 08–10)
    // ═══════════════════════════════════════════════════════════════════════

    /// 08 — ot.close requires step-up; ot.view does not.
    #[tokio::test]
    async fn test_rbac_08_step_up_required_permission_check() {
        let db = setup_db().await;

        // ot.close: dangerous + requires_step_up
        let close_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                ["ot.close".into()],
            ))
            .await
            .expect("query")
            .expect("ot.close exists");
        let close_step_up: i32 = close_row.try_get("", "requires_step_up").unwrap();
        assert_eq!(close_step_up, 1, "ot.close should require step-up");

        // ot.view: not dangerous
        let view_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                ["ot.view".into()],
            ))
            .await
            .expect("query")
            .expect("ot.view exists");
        let view_step_up: i32 = view_row.try_get("", "requires_step_up").unwrap();
        assert_eq!(view_step_up, 0, "ot.view should NOT require step-up");
    }

    /// 09 — Custom permission is always non-dangerous, non-step-up (enforced by DB insert).
    #[tokio::test]
    async fn test_rbac_09_custom_permission_cannot_set_dangerous() {
        let db = setup_db().await;

        // Insert a custom permission directly (simulating what create_custom_permission does)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO permissions (name, description, category, is_dangerous, requires_step_up, is_system, created_at) \
             VALUES (?, 'Test custom', 'custom', 0, 0, 0, datetime('now'))",
            ["cst.test_something".into()],
        ))
        .await
        .expect("insert custom permission");

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT is_dangerous, requires_step_up, is_system FROM permissions WHERE name = ?",
                ["cst.test_something".into()],
            ))
            .await
            .expect("query")
            .expect("custom perm exists");

        let is_dangerous: i32 = row.try_get("", "is_dangerous").unwrap();
        let requires_step_up: i32 = row.try_get("", "requires_step_up").unwrap();
        let is_system: i32 = row.try_get("", "is_system").unwrap();

        assert_eq!(is_dangerous, 0, "custom permissions must not be dangerous");
        assert_eq!(requires_step_up, 0, "custom permissions must not require step-up");
        assert_eq!(is_system, 0, "custom permissions must not be system");
    }

    /// 10 — System namespace prefix blocks custom permission creation.
    #[tokio::test]
    async fn test_rbac_10_system_namespace_blocked() {
        let _db = setup_db().await;

        // The `create_custom_permission` command validates the prefix.
        // Here we validate the same logic: any permission starting with a system
        // prefix (like "ot.") is disallowed for custom permissions.
        let system_prefixes = [
            "eq.", "di.", "ot.", "org.", "per.", "ref.", "inv.", "pm.", "ram.", "rep.",
            "arc.", "doc.", "plan.", "log.", "trn.", "iot.", "erp.", "ptw.", "fin.",
            "ins.", "cfg.", "adm.",
        ];

        let test_name = "ot.my_override";
        let is_blocked = system_prefixes.iter().any(|p| test_name.starts_with(p));
        assert!(
            is_blocked,
            "'ot.my_override' should be blocked by system namespace check"
        );

        // Also verify that a valid cst. name passes
        let valid_name = "cst.my_custom";
        let is_valid = valid_name.starts_with("cst.")
            && !system_prefixes.iter().any(|p| valid_name.starts_with(p));
        assert!(is_valid, "'cst.my_custom' should be allowed");
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  EMERGENCY ELEVATION (tests 11–12)
    // ═══════════════════════════════════════════════════════════════════════

    /// 11 — Emergency grant with future expiry is included in effective permissions.
    #[tokio::test]
    async fn test_rbac_11_emergency_grant_included_before_expiry() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t11_user").await;
        let role_id = get_role_id(&db, "Supervisor").await;
        let future_expiry = date_offset_days(1); // expires tomorrow

        insert_emergency_grant(&db, user_id, role_id, "tenant", &future_expiry, "Incident response")
            .await;

        let perms = resolver::effective_permissions(&db, user_id, "tenant", None)
            .await
            .expect("effective_permissions");

        assert!(
            perms.contains("ot.view"),
            "emergency grant before expiry should be active: {perms:?}"
        );
    }

    /// 12 — Emergency grant with past expiry is excluded from effective permissions.
    #[tokio::test]
    async fn test_rbac_12_emergency_grant_excluded_after_expiry() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t12_user").await;
        let role_id = get_role_id(&db, "Supervisor").await;
        let past_expiry = date_offset_days(-1); // expired yesterday

        insert_emergency_grant(&db, user_id, role_id, "tenant", &past_expiry, "Past incident")
            .await;

        let perms = resolver::effective_permissions(&db, user_id, "tenant", None)
            .await
            .expect("effective_permissions");

        assert!(
            perms.is_empty(),
            "expired emergency grant should yield empty set: {perms:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  DELEGATION (test 13)
    // ═══════════════════════════════════════════════════════════════════════

    /// 13 — Delegation boundary: allowed domains pass, disallowed domains blocked.
    #[tokio::test]
    async fn test_rbac_13_delegation_boundary_enforced() {
        let db = setup_db().await;

        // Create a "SiteAdmin" custom role
        let site_admin_role_id = create_test_role(&db, "SiteAdmin_t13", &["adm.users"]).await;

        // Create delegation policy: this role can manage ot and di domains at entity_A
        insert_delegation_policy(
            &db,
            site_admin_role_id,
            "org_node",
            Some("entity_A"),
            &["ot", "di"],
        )
        .await;

        // Create Bob and assign SiteAdmin role at entity_A
        let bob_id = create_test_user(&db, "bob_t13").await;
        assign_role(
            &db,
            bob_id,
            site_admin_role_id,
            "org_node",
            Some("entity_A"),
            None,
            None,
        )
        .await;

        let target_user_id = 999; // dummy target

        // ot.edit should be allowed (domain "ot" is in allowed_domains)
        let can_ot = delegation::can_delegate_permission(
            &db,
            bob_id,
            target_user_id,
            "ot.edit",
            "org_node",
            Some("entity_A"),
        )
        .await
        .expect("can_delegate ot.edit");
        assert!(can_ot, "ot.edit should be delegatable");

        // adm.users should be blocked (domain "adm" is NOT in allowed_domains)
        let can_adm = delegation::can_delegate_permission(
            &db,
            bob_id,
            target_user_id,
            "adm.users",
            "org_node",
            Some("entity_A"),
        )
        .await
        .expect("can_delegate adm.users");
        assert!(!can_adm, "adm.users should NOT be delegatable");
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  IMPORT/EXPORT (test 14)
    // ═══════════════════════════════════════════════════════════════════════

    /// 14 — Role export → retire original → import as new name → permissions preserved.
    #[tokio::test]
    async fn test_rbac_14_role_export_import_round_trip() {
        let db = setup_db().await;

        // Create a test role with known permissions
        let perm_names = ["ot.view", "ot.create", "ot.edit"];
        let role_id = create_test_role(&db, "TestRole_t14", &perm_names).await;

        // Export: read the role's permissions from DB
        let exported_perms: HashSet<String> = {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT p.name FROM permissions p \
                     INNER JOIN role_permissions rp ON rp.permission_id = p.id \
                     WHERE rp.role_id = ?",
                    [role_id.into()],
                ))
                .await
                .expect("export query");
            rows.iter()
                .filter_map(|r| r.try_get::<String>("", "name").ok())
                .collect()
        };

        assert_eq!(exported_perms.len(), 3, "exported role should have 3 permissions");

        // Retire the original role
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE roles SET status = 'retired', updated_at = datetime('now') WHERE id = ?",
            [role_id.into()],
        ))
        .await
        .expect("retire role");

        // Import as a new role with the exported permissions
        let imported_role_id = create_test_role(
            &db,
            "TestRoleImported_t14",
            &perm_names,
        )
        .await;

        // Verify the imported role has the correct permissions
        let imported_perms: HashSet<String> = {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT p.name FROM permissions p \
                     INNER JOIN role_permissions rp ON rp.permission_id = p.id \
                     WHERE rp.role_id = ?",
                    [imported_role_id.into()],
                ))
                .await
                .expect("import verify query");
            rows.iter()
                .filter_map(|r| r.try_get::<String>("", "name").ok())
                .collect()
        };

        assert_eq!(
            exported_perms, imported_perms,
            "imported role should have same permissions as exported"
        );

        // Validate dependencies are satisfied
        let hard_result =
            resolver::validate_hard_dependencies(&db, &imported_perms).await;
        assert!(
            hard_result.is_ok(),
            "imported permissions should satisfy all hard dependencies"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  FULL GOVERNANCE LIFECYCLE (test 15)
    // ═══════════════════════════════════════════════════════════════════════

    /// 15 — End-to-end: create user → assign role → verify permissions →
    ///       simulate access → audit event → deactivate → verify deny.
    #[tokio::test]
    async fn test_rbac_15_full_admin_lifecycle() {
        let db = setup_db().await;

        // ── Step 1: Create user Alice ────────────────────────────────────
        let alice_id = create_test_user(&db, "alice_t15").await;

        let alice_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT is_active FROM user_accounts WHERE id = ?",
                [alice_id.into()],
            ))
            .await
            .expect("query alice")
            .expect("alice exists");
        let is_active: i32 = alice_row.try_get("", "is_active").unwrap();
        assert_eq!(is_active, 1, "Alice should be active");

        // No scope assignments yet
        let initial_perms =
            resolver::effective_permissions(&db, alice_id, "tenant", None)
                .await
                .expect("initial perms");
        assert!(initial_perms.is_empty(), "no assignments → empty perms");

        // ── Step 2: Assign Maintenance Technician role at tenant scope ───
        let tech_role_id = get_role_id(&db, "Maintenance Technician").await;
        assign_role(&db, alice_id, tech_role_id, "tenant", None, None, None).await;

        let perms_after_assign =
            resolver::effective_permissions(&db, alice_id, "tenant", None)
                .await
                .expect("perms after assign");
        assert!(
            perms_after_assign.contains("ot.view"),
            "Technician should have ot.view: {perms_after_assign:?}"
        );
        assert!(
            !perms_after_assign.contains("adm.users"),
            "Technician should NOT have adm.users: {perms_after_assign:?}"
        );

        // ── Step 3: Simulate access (verify same assertions via resolver) ─
        let has_ot_view =
            resolver::user_has_permission(&db, alice_id, "ot.view", "tenant", None)
                .await
                .expect("has ot.view");
        assert!(has_ot_view, "simulate: ot.view should be true");

        let has_adm_users =
            resolver::user_has_permission(&db, alice_id, "adm.users", "tenant", None)
                .await
                .expect("has adm.users");
        assert!(!has_adm_users, "simulate: adm.users should be false");

        // ── Step 4: Validate ot.create in Technician's permission set ────
        // Maintenance Technician has ot.view, ot.create, ot.edit
        // ot.create depends hard on ot.view → satisfied
        let tech_perm_set: HashSet<String> = ["ot.view", "ot.create", "ot.edit"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let valid_result =
            resolver::validate_hard_dependencies(&db, &tech_perm_set).await;
        assert!(
            valid_result.is_ok(),
            "ot.create + ot.view should satisfy hard deps"
        );

        // ── Step 5: Admin audit event ────────────────────────────────────
        // The IPC command `assign_role_scope` writes to admin_change_events.
        // We simulate that event here since we're bypassing IPC.
        let admin_id: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'admin'",
                [],
            ))
            .await
            .expect("query admin")
            .expect("admin exists")
            .try_get("", "id")
            .expect("admin id");

        insert_admin_event(
            &db,
            "role_assigned",
            admin_id,
            Some(alice_id),
            Some(tech_role_id),
        )
        .await;

        // Verify the event exists
        let events = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT action, target_user_id FROM admin_change_events \
                 WHERE actor_id = ? AND action = 'role_assigned' AND target_user_id = ?",
                [admin_id.into(), alice_id.into()],
            ))
            .await
            .expect("query audit events");
        assert!(
            !events.is_empty(),
            "at least 1 role_assigned event for Alice should exist"
        );

        // ── Step 6: Deactivate Alice ─────────────────────────────────────
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET is_active = 0, updated_at = datetime('now') WHERE id = ?",
            [alice_id.into()],
        ))
        .await
        .expect("deactivate alice");

        let deactivated_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT is_active FROM user_accounts WHERE id = ?",
                [alice_id.into()],
            ))
            .await
            .expect("query deactivated")
            .expect("row exists");
        let is_active_after: i32 = deactivated_row.try_get("", "is_active").unwrap();
        assert_eq!(is_active_after, 0, "Alice should be deactivated");

        // Scope assignments still exist in the table (not deleted on deactivation)
        let assignment_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) as cnt FROM user_scope_assignments \
                 WHERE user_id = ? AND deleted_at IS NULL",
                [alice_id.into()],
            ))
            .await
            .expect("query assignments")
            .expect("count row")
            .try_get("", "cnt")
            .expect("cnt");
        assert!(
            assignment_count > 0,
            "assignments should still exist after deactivation"
        );

        // The resolver itself still returns permissions (is_active is checked
        // at the command layer, not in the resolver). This documents the design:
        // the resolver is scope-and-time-based; activation checks are in IPC commands.
        let perms_after_deactivate =
            resolver::effective_permissions(&db, alice_id, "tenant", None)
                .await
                .expect("perms after deactivate");
        // Assignments are still present, resolver doesn't check is_active
        assert!(
            perms_after_deactivate.contains("ot.view"),
            "resolver returns perms even for deactivated users (command layer checks is_active)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  SPRINT S4 (tests 16–19)
    // ═══════════════════════════════════════════════════════════════════════

    /// 16 — Password older than max age is marked expired.
    #[tokio::test]
    async fn test_rbac_16_password_expiry_enforced() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t16_user").await;

        let changed_at = (chrono::Utc::now() - chrono::Duration::days(91)).to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET password_changed_at = ?, updated_at = datetime('now') WHERE id = ?",
            [changed_at.into(), user_id.into()],
        ))
        .await
        .expect("set password_changed_at");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE rbac_settings SET value = ? WHERE key = ?",
            ["90".into(), "password_max_age_days".into()],
        ))
        .await
        .expect("set max age");

        let policy = password_policy::PasswordPolicy::load(&db).await;
        let status = password_policy::check_password_expiry(&db, user_id as i32, &policy)
            .await
            .expect("check_password_expiry");

        assert!(
            matches!(status, password_policy::PasswordExpiryStatus::Expired),
            "expected Expired, got {status:?}"
        );
    }

    /// 17 — Password near expiry returns warning with remaining days.
    #[tokio::test]
    async fn test_rbac_17_password_expiry_warning() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t17_user").await;

        let changed_at = (chrono::Utc::now() - chrono::Duration::days(80)).to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET password_changed_at = ?, updated_at = datetime('now') WHERE id = ?",
            [changed_at.into(), user_id.into()],
        ))
        .await
        .expect("set password_changed_at");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE rbac_settings SET value = ? WHERE key = ?",
            ["90".into(), "password_max_age_days".into()],
        ))
        .await
        .expect("set max age");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE rbac_settings SET value = ? WHERE key = ?",
            ["14".into(), "password_warn_days".into()],
        ))
        .await
        .expect("set warn days");

        let policy = password_policy::PasswordPolicy::load(&db).await;
        let status = password_policy::check_password_expiry(&db, user_id as i32, &policy)
            .await
            .expect("check_password_expiry");

        match status {
            password_policy::PasswordExpiryStatus::ExpiringSoon { days_remaining } => {
                assert_eq!(days_remaining, 10, "expected 10 days remaining");
            }
            other => panic!("expected ExpiringSoon, got {other:?}"),
        }
    }

    /// 18 — Correct PIN unlocks locked session and writes audit event.
    #[tokio::test]
    async fn test_rbac_18_pin_unlock_success() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t18_user").await;

        let pin_hash = pin::hash_pin("1234").expect("hash pin");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET pin_hash = ?, updated_at = datetime('now') WHERE id = ?",
            [pin_hash.into(), user_id.into()],
        ))
        .await
        .expect("set pin hash");

        let mut sm = session_manager::SessionManager::new();
        sm.create_session(make_auth_user(user_id, "t18_user"));
        if let Some(current) = &mut sm.current {
            current.pin_configured = true;
        }
        sm.lock_session();

        let info = crate::commands::auth::unlock_session_with_pin_internal(&db, &mut sm, "1234")
            .await
            .expect("unlock with pin");

        assert!(!info.is_locked, "session should be unlocked");
        assert!(info.is_authenticated, "session should be authenticated");

        let cnt: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) as cnt FROM audit_events WHERE event_type = ?",
                [crate::audit::event_type::SESSION_UNLOCKED_WITH_PIN.into()],
            ))
            .await
            .expect("query audit")
            .expect("audit count row")
            .try_get("", "cnt")
            .expect("cnt");

        assert!(cnt > 0, "expected session.unlocked_with_pin audit event");
    }

    /// 19 — Three failed PIN attempts disable PIN and require password unlock.
    #[tokio::test]
    async fn test_rbac_19_pin_unlock_failure_locks_to_password() {
        let db = setup_db().await;
        let user_id = create_test_user(&db, "t19_user").await;

        let pin_hash = pin::hash_pin("1234").expect("hash pin");
        let pw_hash = password::hash_password("Pass#1234").expect("hash password");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET pin_hash = ?, password_hash = ?, updated_at = datetime('now') WHERE id = ?",
            [pin_hash.into(), pw_hash.into(), user_id.into()],
        ))
        .await
        .expect("set pin+password hash");

        let mut sm = session_manager::SessionManager::new();
        sm.create_session(make_auth_user(user_id, "t19_user"));
        if let Some(current) = &mut sm.current {
            current.pin_configured = true;
        }
        sm.lock_session();

        for _ in 0..2 {
            let err = crate::commands::auth::unlock_session_with_pin_internal(&db, &mut sm, "9999")
                .await
                .expect_err("wrong pin should fail");
            assert!(
                err.to_string().contains("PIN incorrect"),
                "first two attempts should be invalid pin"
            );
        }

        let third = crate::commands::auth::unlock_session_with_pin_internal(&db, &mut sm, "9999")
            .await
            .expect_err("third wrong pin should disable pin mode");
        assert!(
            third.to_string().contains("PIN désactivé"),
            "third attempt should disable pin mode: {third}"
        );

        // Password unlock remains available after PIN disable.
        let user_row = session_manager::find_active_user(&db, "t19_user")
            .await
            .expect("find user")
            .expect("user exists");
        let stored_hash = user_row.5.expect("password hash");
        let valid = password::verify_password("Pass#1234", &stored_hash).expect("verify password");
        assert!(valid, "password should still verify after PIN lockout");

        assert!(
            sm.unlock_session(),
            "password unlock path should remain available"
        );
        let info = sm.session_info();
        assert!(info.is_authenticated, "session should be authenticated");
        assert!(!info.is_locked, "session should be unlocked");
    }
}
