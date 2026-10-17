//! Site-scoped access (step 6): plant-wide, site-only, wrong-site and no-anchor cases on a small
//! org tree (plant → site A → line A1, plant → site B).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::auth::scope::{self, AccessScope, Anchor};
use crate::errors::AppError;
use crate::rbac::permissions::{ADM_USERS, EQ_VIEW};

const NOW: &str = "2026-01-01T00:00:00Z";

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect(sql);
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("value")
}

struct Tree {
    plant: i64,
    site_a: i64,
    line_a1: i64,
    site_b: i64,
}

async fn node(db: &DatabaseConnection, code: &str, parent: Option<i64>) -> i64 {
    let parent_sql = parent.map_or_else(|| "NULL".to_string(), |p| p.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
                created_at, updated_at, row_version, structure_model_id)
             SELECT 'scope-{code}', 'SC-{code}', 'Node {code}', node_type_id, {parent_sql}, '/', 0, 'active',
                    '{NOW}', '{NOW}', 1, structure_model_id FROM org_nodes ORDER BY id LIMIT 1"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM org_nodes WHERE entity_sync_id = 'scope-{code}'"),
    )
    .await
}

async fn tree(db: &DatabaseConnection) -> Tree {
    let plant = node(db, "PLANT", None).await;
    let site_a = node(db, "A", Some(plant)).await;
    let line_a1 = node(db, "A1", Some(site_a)).await;
    let site_b = node(db, "B", Some(plant)).await;
    Tree {
        plant,
        site_a,
        line_a1,
        site_b,
    }
}

async fn user(db: &DatabaseConnection, name: &str) -> i32 {
    exec(
        db,
        &format!(
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin,
                force_password_change, failed_login_attempts, created_at, updated_at, row_version)
             VALUES ('scope-user-{name}', '{name}', '{name}', 'local', 1, 0, 0, 0, '{NOW}', '{NOW}', 1)"
        ),
    )
    .await;
    i32::try_from(scalar(db, &format!("SELECT id FROM user_accounts WHERE username = '{name}'")).await).expect("id")
}

async fn role(db: &DatabaseConnection, name: &str, permissions: &[&str]) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO roles (entity_sync_id, name, description, role_type, status, is_system, created_at, updated_at, row_version)
             VALUES ('scope-role-{name}', '{name}', NULL, 'custom', 'active', 0, '{NOW}', '{NOW}', 1)"
        ),
    )
    .await;
    let id = scalar(db, &format!("SELECT id FROM roles WHERE name = '{name}'")).await;
    for p in permissions {
        exec(
            db,
            &format!(
                "INSERT INTO role_permissions (role_id, permission_id, granted_at)
                 SELECT {id}, id, '{NOW}' FROM permissions WHERE name = '{p}'"
            ),
        )
        .await;
    }
    id
}

async fn assign(db: &DatabaseConnection, user_id: i32, role_id: i64, scope_type: &str, reference: Option<i64>) {
    let reference = reference.map_or_else(|| "NULL".to_string(), |r| format!("'{r}'"));
    exec(
        db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
                created_at, updated_at, row_version)
             VALUES ('scope-usa-{user_id}-{role_id}-{scope_type}', {user_id}, {role_id}, '{scope_type}', {reference},
                     '{NOW}', '{NOW}', 1)"
        ),
    )
    .await;
}

fn code_of(err: &AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        AppError::PermissionDenied(_) => "PERMISSION_DENIED".to_string(),
        other => format!("{other:?}"),
    }
}

/// The anchors among `nodes` (a `None` is a row without a site) that the list predicate keeps.
async fn filtered(db: &DatabaseConnection, scope: &AccessScope, nodes: &[Option<i64>]) -> Vec<Option<i64>> {
    let values = nodes
        .iter()
        .map(|n| n.map_or_else(|| "(NULL)".to_string(), |id| format!("({id})")))
        .collect::<Vec<_>>()
        .join(",");
    let (predicate, binds) = scope::scope_filter_sql(scope, "v.anchor");
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("WITH v(anchor) AS (VALUES {values}) SELECT anchor FROM v WHERE {predicate} ORDER BY anchor"),
            binds,
        ))
        .await
        .expect("filter");
    rows.iter()
        .map(|r| r.try_get::<Option<i64>>("", "anchor").expect("anchor"))
        .collect()
}

#[tokio::test]
async fn a_plant_wide_role_grants_everywhere_including_records_without_a_site() {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let u = user(&db, "plant_user").await;
    let r = role(&db, "Scope viewer plant", &[EQ_VIEW]).await;
    assign(&db, u, r, "tenant", None).await;

    let s = scope::access_scope(&db, u, EQ_VIEW).await.expect("scope");
    assert_eq!(s, AccessScope::Everywhere);
    for anchor in [
        Anchor::Node(t.site_b),
        Anchor::Node(t.line_a1),
        Anchor::Unknown,
        Anchor::Plant,
    ] {
        scope::check_anchor(&db, &s, EQ_VIEW, anchor).await.expect("plant-wide");
    }
    let all = [Some(t.plant), Some(t.site_a), Some(t.line_a1), Some(t.site_b), None];
    assert_eq!(filtered(&db, &s, &all).await.len(), 5, "the predicate keeps every row");
}

#[tokio::test]
async fn a_site_role_grants_on_its_site_and_below_and_refuses_the_rest_with_a_reason() {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let u = user(&db, "site_user").await;
    let r = role(&db, "Scope viewer site", &[EQ_VIEW]).await;
    assign(&db, u, r, "org_node", Some(t.site_a)).await;

    let s = scope::access_scope(&db, u, EQ_VIEW).await.expect("scope");
    assert_eq!(s, AccessScope::Subtrees([t.site_a].into()));
    scope::check_anchor(&db, &s, EQ_VIEW, Anchor::Node(t.site_a))
        .await
        .expect("its site");
    scope::check_anchor(&db, &s, EQ_VIEW, Anchor::Node(t.line_a1))
        .await
        .expect("a line below its site");

    let wrong = scope::check_anchor(&db, &s, EQ_VIEW, Anchor::Node(t.site_b))
        .await
        .expect_err("another site");
    assert_eq!(code_of(&wrong), "SCOPE_OUTSIDE_SITE");
    let above = scope::check_anchor(&db, &s, EQ_VIEW, Anchor::Node(t.plant))
        .await
        .expect_err("the plant above its site");
    assert_eq!(code_of(&above), "SCOPE_OUTSIDE_SITE");
    let none = scope::check_anchor(&db, &s, EQ_VIEW, Anchor::Unknown)
        .await
        .expect_err("no site");
    assert_eq!(code_of(&none), "SCOPE_RECORD_WITHOUT_SITE");
    assert_eq!(Anchor::of(Some(0)), Anchor::Unknown, "an old 0 node is no site");
    assert_eq!(Anchor::of(None), Anchor::Unknown);
    let plant = scope::check_anchor(&db, &s, EQ_VIEW, Anchor::Plant)
        .await
        .expect_err("plant-wide object");
    assert_eq!(code_of(&plant), "SCOPE_PLANT_WIDE_ONLY");

    // A permission the role does not carry: refused everywhere, as before.
    let other = scope::access_scope(&db, u, ADM_USERS).await.expect("scope");
    assert!(!other.is_anywhere());
    let denied = scope::check_anchor(&db, &other, ADM_USERS, Anchor::Node(t.site_a))
        .await
        .expect_err("not held");
    assert_eq!(code_of(&denied), "PERMISSION_DENIED");

    // Lists: its site and the line below; never the other site, the plant, or a row without site.
    let all = [Some(t.plant), Some(t.site_a), Some(t.line_a1), Some(t.site_b), None];
    let mut expected = vec![Some(t.site_a), Some(t.line_a1)];
    expected.sort();
    assert_eq!(filtered(&db, &s, &all).await, expected);
    assert!(filtered(&db, &other, &all).await.is_empty(), "nowhere: nothing");

    // The plant-wide check of modules not yet converted still refuses a site role.
    assert!(
        !crate::auth::rbac::check_permission(&db, u, EQ_VIEW, &crate::auth::rbac::PermissionScope::Global)
            .await
            .expect("check"),
        "unchanged plant-wide check"
    );
}

#[tokio::test]
async fn the_screens_read_rights_per_node_and_a_site_only_user_signs_in() {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let u = user(&db, "site_screen").await;
    let r = role(&db, "Scope screen site", &[EQ_VIEW]).await;
    assign(&db, u, r, "org_node", Some(t.site_a)).await;

    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let map = scope::access_map_cached(&db, &cache, u).await.expect("map");
    assert_eq!(cache.read().await.get_access(i64::from(u)), Some(&map), "cached");
    let mine = scope::my_access(&map);
    assert!(!mine.all);
    assert!(mine.everywhere.is_empty());
    assert_eq!(mine.by_node.get(&t.site_a), Some(&vec![EQ_VIEW.to_string()]));
    assert_eq!(
        scope::permissions_at_node(&db, &map, t.line_a1).await.expect("at"),
        Some(vec![EQ_VIEW.to_string()]),
        "held at the line below the site"
    );
    assert_eq!(
        scope::permissions_at_node(&db, &map, t.site_b).await.expect("at"),
        Some(Vec::new()),
        "nothing at the other site"
    );
    cache.write().await.invalidate_user(i64::from(u));
    assert!(
        cache.read().await.get_access(i64::from(u)).is_none(),
        "cleared with the user's roles"
    );

    // Sign-in: a site-only user is accepted on the activated tenant; a user with no role is not.
    let resolved = crate::auth::session_manager::resolve_tenant_scope_for_user(&db, u, "tenant-x")
        .await
        .expect("resolve");
    assert_eq!(resolved.map(|r| r.tenant_id), Some("tenant-x".to_string()));
    let nobody = user(&db, "no_role").await;
    assert!(
        crate::auth::session_manager::resolve_tenant_scope_for_user(&db, nobody, "tenant-x")
            .await
            .expect("resolve")
            .is_none()
    );
}

#[tokio::test]
async fn only_live_node_roles_grant_and_let_a_site_only_user_sign_in() {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let r = role(&db, "Scope edge cases", &[EQ_VIEW]).await;
    let sign_in = |u: i32| {
        let db = db.clone();
        async move {
            crate::auth::session_manager::resolve_tenant_scope_for_user(&db, u, "tenant-x")
                .await
                .expect("resolve")
                .is_some()
        }
    };

    // Ended, lapsed emergency, team-scoped, malformed reference, deleted node: nothing granted,
    // no sign-in.
    let ended = user(&db, "edge_ended").await;
    assign(&db, ended, r, "org_node", Some(t.site_a)).await;
    exec(&db, &format!("UPDATE user_scope_assignments SET valid_to = '2020-01-01T00:00:00Z' WHERE user_id = {ended}")).await;
    let lapsed = user(&db, "edge_lapsed").await;
    assign(&db, lapsed, r, "org_node", Some(t.site_a)).await;
    exec(
        &db,
        &format!(
            "UPDATE user_scope_assignments SET is_emergency = 1, emergency_expires_at = '2020-01-01T00:00:00Z'
              WHERE user_id = {lapsed}"
        ),
    )
    .await;
    let team = user(&db, "edge_team").await;
    assign(&db, team, r, "team", Some(1)).await;
    let malformed = user(&db, "edge_malformed").await;
    assign(&db, malformed, r, "org_node", Some(t.site_a)).await;
    exec(&db, &format!("UPDATE user_scope_assignments SET scope_reference = 'abc' WHERE user_id = {malformed}")).await;
    let gone = node(&db, "GONE", Some(t.plant)).await;
    let on_gone = user(&db, "edge_gone").await;
    assign(&db, on_gone, r, "org_node", Some(gone)).await;
    exec(&db, &format!("UPDATE org_nodes SET deleted_at = '{NOW}' WHERE id = {gone}")).await;
    for (label, u) in [("ended", ended), ("lapsed emergency", lapsed), ("team", team), ("malformed", malformed), ("deleted node", on_gone)] {
        let s = scope::access_scope(&db, u, EQ_VIEW).await.expect("scope");
        assert!(!s.is_anywhere(), "{label}: grants nothing, got {s:?}");
        assert!(!sign_in(u).await, "{label}: no sign-in");
    }

    // An emergency role on a site still in force grants there, like any other.
    let urgent = user(&db, "edge_urgent").await;
    assign(&db, urgent, r, "org_node", Some(t.site_a)).await;
    exec(
        &db,
        &format!(
            "UPDATE user_scope_assignments SET is_emergency = 1, emergency_expires_at = '2099-01-01T00:00:00Z'
              WHERE user_id = {urgent}"
        ),
    )
    .await;
    assert_eq!(
        scope::access_scope(&db, urgent, EQ_VIEW).await.expect("scope"),
        AccessScope::Subtrees([t.site_a].into())
    );

    // The older `site` and `entity` scope types count as a node like `org_node`.
    let legacy = user(&db, "edge_legacy").await;
    let r2 = role(&db, "Scope legacy types", &[EQ_VIEW]).await;
    assign(&db, legacy, r, "site", Some(t.site_a)).await;
    assign(&db, legacy, r2, "entity", Some(t.site_b)).await;
    assert_eq!(
        scope::access_scope(&db, legacy, EQ_VIEW).await.expect("scope"),
        AccessScope::Subtrees([t.site_a, t.site_b].into())
    );

    // Held plant-wide and on a site: plant-wide wins.
    let both = user(&db, "edge_both").await;
    assign(&db, both, r, "org_node", Some(t.site_a)).await;
    assign(&db, both, r2, "tenant", None).await;
    assert_eq!(scope::access_scope(&db, both, EQ_VIEW).await.expect("scope"), AccessScope::Everywhere);

    // A plant-wide role tied to another tenant keeps refusing sign-in, even with a site role here.
    let other_tenant = user(&db, "edge_other_tenant").await;
    assign(&db, other_tenant, r2, "tenant", None).await;
    exec(
        &db,
        &format!("UPDATE user_scope_assignments SET scope_reference = 'tenant-other' WHERE user_id = {other_tenant}"),
    )
    .await;
    assign(&db, other_tenant, r, "org_node", Some(t.site_a)).await;
    assert!(!sign_in(other_tenant).await, "the explicit tenant rule still applies");
}

#[tokio::test]
async fn administrator_roles_stay_plant_wide() {
    let db = crate::db::test_db::seeded().await;
    let admin_role = scalar(
        &db,
        "SELECT id FROM roles WHERE name = 'Administrator' AND deleted_at IS NULL",
    )
    .await;
    let refused = crate::commands::admin_users::ensure_role_allowed_at_scope(&db, admin_role, "org_node")
        .await
        .expect_err("administrator on a node");
    assert_eq!(code_of(&refused), "SCOPE_ADMIN_ROLE_PLANT_WIDE");
    crate::commands::admin_users::ensure_role_allowed_at_scope(&db, admin_role, "tenant")
        .await
        .expect("plant-wide administrator");
    let viewer = role(&db, "Scope viewer any", &[EQ_VIEW]).await;
    crate::commands::admin_users::ensure_role_allowed_at_scope(&db, viewer, "org_node")
        .await
        .expect("other roles on a node");
}
