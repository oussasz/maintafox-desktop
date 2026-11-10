//! Site scope of PM plans: on the org tree of `auth::scope_tests` (plant → site A → line A1,
//! plant → site B), a plant-wide planner is unchanged; a planner whose role is on site A creates,
//! edits, publishes, approves and deletes the plans of site A (equipment there, or a location at or
//! below it) and is refused those of site B and the plant-wide family / criticality-group plans; a
//! plan moves to another site only as a draft, with the rights at both ends, and the move is
//! recorded with the update.

use sea_orm::DatabaseConnection;

use crate::auth::scope::{self, AccessMap, Anchor};
use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::errors::AppError;
use crate::pm::access::{self, PlanSiteMove};
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, TransitionPmPlanLifecycleInput, UpdatePmPlanInput,
};
use crate::pm::queries;
use crate::rbac::permissions::{PM_APPROVE, PM_CREATE, PM_DELETE, PM_EDIT, PM_VIEW};

/// Every PM plan right.
const ALL_PM: &[&str] = &[PM_VIEW, PM_CREATE, PM_EDIT, PM_APPROVE, PM_DELETE];

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// Every PM right, plant-wide.
    plant: i32,
    /// Every PM right, on site A only.
    site_a: i32,
    /// Every PM right, on site A and on site B (two node roles).
    sites_ab: i32,
    /// Equipment installed on site A, line A1, site B, and not installed anywhere.
    eq_a: i64,
    eq_a1: i64,
    eq_b: i64,
    eq_none: i64,
    /// Draft plans: on equipment of site A, of site B, on the location line A1, on an equipment
    /// family and on a criticality group.
    plan_a: i64,
    plan_b: i64,
    plan_loc_a1: i64,
    plan_family: i64,
    plan_group: i64,
    /// A draft version of plan A and of plan B.
    version_a: i64,
    version_b: i64,
}

async fn equipment(db: &DatabaseConnection, code: &str, node: Option<i64>) -> i64 {
    let node_sql = node.map_or_else(|| "NULL".to_string(), |n| n.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('pm-scope-eq-{code}', 'PM-SCOPE-{code}', 'Equipment {code}', 'active_in_service', {node_sql},
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'pm-scope-eq-{code}'"),
    )
    .await
}

fn plan_input(code: &str, scope_type: &str, scope_id: Option<i64>) -> CreatePmPlanInput {
    CreatePmPlanInput {
        code: code.to_string(),
        title: format!("{code} (site scope test)"),
        description: None,
        asset_scope_type: scope_type.to_string(),
        asset_scope_id: scope_id,
        strategy_type: "fixed".to_string(),
        criticality_value_id: None,
        assigned_team_id: None,
        requires_shutdown: false,
        requires_permit: false,
        is_active: Some(true),
    }
}

async fn plan(db: &DatabaseConnection, code: &str, scope_type: &str, scope_id: Option<i64>) -> i64 {
    queries::create_pm_plan(db, plan_input(code, scope_type, scope_id))
        .await
        .expect("create plan")
        .id
}

async fn version(db: &DatabaseConnection, plan_id: i64) -> i64 {
    queries::create_pm_plan_version(
        db,
        plan_id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            trigger_definition_json: r#"{"interval_unit":"day","interval_value":30}"#.to_string(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".to_string()),
            required_tools_json: None,
            estimated_duration_hours: None,
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("create version")
    .id
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let plant = user(&db, "pm_plant").await;
    let site_a = user(&db, "pm_site_a").await;
    let sites_ab = user(&db, "pm_sites_ab").await;
    let plant_role = role(&db, "PM scope plant", ALL_PM).await;
    let site_role = role(&db, "PM scope site", ALL_PM).await;
    assign(&db, plant, plant_role, "tenant", None).await;
    assign(&db, site_a, site_role, "org_node", Some(t.site_a)).await;
    assign(&db, sites_ab, site_role, "org_node", Some(t.site_a)).await;
    // A second assignment of the same role (the fixture's sync id names the scope type: use `site`).
    assign(&db, sites_ab, site_role, "site", Some(t.site_b)).await;

    let eq_a = equipment(&db, "A", Some(t.site_a)).await;
    let eq_a1 = equipment(&db, "A1", Some(t.line_a1)).await;
    let eq_b = equipment(&db, "B", Some(t.site_b)).await;
    let eq_none = equipment(&db, "NONE", None).await;
    let plan_a = plan(&db, "PM-SCOPE-A", "equipment", Some(eq_a)).await;
    let plan_b = plan(&db, "PM-SCOPE-B", "equipment", Some(eq_b)).await;
    let plan_loc_a1 = plan(&db, "PM-SCOPE-LOC-A1", "location", Some(t.line_a1)).await;
    let plan_family = plan(&db, "PM-SCOPE-FAMILY", "family", None).await;
    let plan_group = plan(&db, "PM-SCOPE-GROUP", "criticality_group", None).await;
    let version_a = version(&db, plan_a).await;
    let version_b = version(&db, plan_b).await;
    Fixture {
        db,
        t,
        plant,
        site_a,
        sites_ab,
        eq_a,
        eq_a1,
        eq_b,
        eq_none,
        plan_a,
        plan_b,
        plan_loc_a1,
        plan_family,
        plan_group,
        version_a,
        version_b,
    }
}

/// The user's rights now (a fresh cache: roles change within a test).
async fn map_of(db: &DatabaseConnection, user_id: i32) -> AccessMap {
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    scope::access_map_cached(db, &cache, user_id).await.expect("access map")
}

fn gate_of(err: &AppError) -> String {
    match err {
        AppError::ValidationFailed(msgs) => msgs
            .first()
            .and_then(|m| m.split([':', ' ']).next())
            .unwrap_or_default()
            .to_string(),
        other => code_of(other),
    }
}

fn no_change() -> UpdatePmPlanInput {
    UpdatePmPlanInput {
        title: None,
        description: None,
        asset_scope_type: None,
        asset_scope_id: None,
        strategy_type: None,
        criticality_value_id: None,
        assigned_team_id: None,
        requires_shutdown: None,
        requires_permit: None,
        is_active: None,
    }
}

async fn row_version(db: &DatabaseConnection, plan_id: i64) -> i64 {
    queries::get_pm_plan(db, plan_id).await.expect("plan").row_version
}

async fn propose(db: &DatabaseConnection, plan_id: i64) {
    queries::transition_pm_plan_lifecycle(
        db,
        TransitionPmPlanLifecycleInput {
            plan_id,
            expected_row_version: row_version(db, plan_id).await,
            next_status: "proposed".to_string(),
        },
    )
    .await
    .expect("propose");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Anchors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_plan_is_anchored_at_its_equipment_or_location_and_family_and_group_plans_are_plant_wide() {
    let f = fixture().await;
    let db = &f.db;
    assert_eq!(
        access::plan_anchor(db, f.plan_a).await.unwrap(),
        Anchor::Node(f.t.site_a)
    );
    assert_eq!(
        access::plan_anchor(db, f.plan_b).await.unwrap(),
        Anchor::Node(f.t.site_b)
    );
    assert_eq!(
        access::plan_anchor(db, f.plan_loc_a1).await.unwrap(),
        Anchor::Node(f.t.line_a1)
    );
    assert_eq!(access::plan_anchor(db, f.plan_family).await.unwrap(), Anchor::Plant);
    assert_eq!(access::plan_anchor(db, f.plan_group).await.unwrap(), Anchor::Plant);
    assert_eq!(
        access::target_anchor(db, "equipment", Some(f.eq_none)).await.unwrap(),
        Anchor::Unknown
    );
    assert!(matches!(
        access::plan_anchor(db, 9_999_999).await,
        Err(AppError::NotFound { .. })
    ));

    // The read model shows the same node to the screens (`None`: plant-wide roles only).
    for (plan_id, expected) in [
        (f.plan_a, Some(f.t.site_a)),
        (f.plan_b, Some(f.t.site_b)),
        (f.plan_loc_a1, Some(f.t.line_a1)),
        (f.plan_family, None),
        (f.plan_group, None),
    ] {
        let one = queries::get_pm_plan(db, plan_id).await.expect("plan");
        assert_eq!(one.access_node_id, expected, "get {plan_id}");
    }
    let listed = queries::list_pm_plans(
        db,
        crate::pm::domain::PmPlanFilter {
            search: None,
            strategy_type: None,
            lifecycle_status: None,
            is_active: None,
        },
    )
    .await
    .expect("list");
    let a = listed.iter().find(|p| p.id == f.plan_a).expect("plan A listed");
    assert_eq!(a.access_node_id, Some(f.t.site_a));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Create
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_site_planner_creates_plans_at_their_site_only() {
    let f = fixture().await;
    let db = &f.db;
    let site = map_of(db, f.site_a).await;
    access::check_create_at(db, &site, "equipment", Some(f.eq_a))
        .await
        .expect("equipment of site A");
    access::check_create_at(db, &site, "equipment", Some(f.eq_a1))
        .await
        .expect("equipment below site A");
    access::check_create_at(db, &site, "location", Some(f.t.line_a1))
        .await
        .expect("location below site A");
    access::check_create_at(db, &site, "location", Some(f.t.site_a))
        .await
        .expect("site A itself");

    let other = access::check_create_at(db, &site, "equipment", Some(f.eq_b))
        .await
        .expect_err("site B");
    assert_eq!(code_of(&other), "SCOPE_OUTSIDE_SITE");
    let above = access::check_create_at(db, &site, "location", Some(f.t.plant))
        .await
        .expect_err("the plant node above site A");
    assert_eq!(code_of(&above), "SCOPE_OUTSIDE_SITE");
    for kind in ["family", "criticality_group"] {
        let err = access::check_create_at(db, &site, kind, Some(1)).await.expect_err(kind);
        assert_eq!(code_of(&err), "SCOPE_PLANT_WIDE_ONLY", "{kind}");
    }
    let nowhere = access::check_create_at(db, &site, "equipment", Some(f.eq_none))
        .await
        .expect_err("equipment installed nowhere");
    assert_eq!(code_of(&nowhere), "SCOPE_RECORD_WITHOUT_SITE");
}

#[tokio::test]
async fn a_plant_wide_planner_creates_every_kind_of_plan() {
    let f = fixture().await;
    let db = &f.db;
    let plant = map_of(db, f.plant).await;
    for (kind, id) in [
        ("equipment", Some(f.eq_a)),
        ("equipment", Some(f.eq_b)),
        ("equipment", Some(f.eq_none)),
        ("location", Some(f.t.plant)),
        ("family", None),
        ("criticality_group", None),
    ] {
        access::check_create_at(db, &plant, kind, id)
            .await
            .unwrap_or_else(|e| panic!("{kind} {id:?}: {e:?}"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Edit, publish, approve, delete
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_site_planner_edits_publishes_approves_and_deletes_at_their_site_only() {
    let f = fixture().await;
    let db = &f.db;
    let site = map_of(db, f.site_a).await;
    for permission in [PM_EDIT, PM_APPROVE, PM_DELETE, PM_CREATE] {
        access::check_on_plan(db, &site, permission, f.plan_a)
            .await
            .unwrap_or_else(|e| panic!("{permission} on plan A: {e:?}"));
        access::check_on_plan(db, &site, permission, f.plan_loc_a1)
            .await
            .unwrap_or_else(|e| panic!("{permission} on the A1 location plan: {e:?}"));
        let b = access::check_on_plan(db, &site, permission, f.plan_b)
            .await
            .expect_err("plan of site B");
        assert_eq!(code_of(&b), "SCOPE_OUTSIDE_SITE", "{permission}");
        for plant_wide in [f.plan_family, f.plan_group] {
            let err = access::check_on_plan(db, &site, permission, plant_wide)
                .await
                .expect_err("plant-wide plan");
            assert_eq!(code_of(&err), "SCOPE_PLANT_WIDE_ONLY", "{permission}");
        }
    }
    // Versions (edit, publish, delete) are checked at their plan.
    for permission in [PM_EDIT, PM_DELETE] {
        access::check_on_version(db, &site, permission, f.version_a)
            .await
            .unwrap_or_else(|e| panic!("{permission} on version A: {e:?}"));
        let b = access::check_on_version(db, &site, permission, f.version_b)
            .await
            .expect_err("version of site B");
        assert_eq!(code_of(&b), "SCOPE_OUTSIDE_SITE", "{permission}");
    }
    assert!(matches!(
        access::check_on_version(db, &site, PM_EDIT, 9_999_999).await,
        Err(AppError::NotFound { .. })
    ));
}

#[tokio::test]
async fn a_plant_wide_planner_acts_on_every_plan() {
    let f = fixture().await;
    let db = &f.db;
    let plant = map_of(db, f.plant).await;
    for permission in [PM_EDIT, PM_APPROVE, PM_DELETE, PM_CREATE] {
        for plan_id in [f.plan_a, f.plan_b, f.plan_loc_a1, f.plan_family, f.plan_group] {
            access::check_on_plan(db, &plant, permission, plan_id)
                .await
                .unwrap_or_else(|e| panic!("{permission} on {plan_id}: {e:?}"));
        }
    }
    access::check_on_version(db, &plant, PM_EDIT, f.version_b)
        .await
        .expect("version B");
}

#[tokio::test]
async fn a_user_without_the_right_anywhere_is_refused_as_before() {
    let f = fixture().await;
    let db = &f.db;
    let nobody = user(db, "pm_nobody").await;
    let none = map_of(db, nobody).await;
    let err = access::check_on_plan(db, &none, PM_EDIT, f.plan_a)
        .await
        .expect_err("no right");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    let err = access::check_create_at(db, &none, "equipment", Some(f.eq_a))
        .await
        .expect_err("no right");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Changing a plan's site
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn an_edit_that_keeps_the_site_needs_the_right_there_only() {
    let f = fixture().await;
    let db = &f.db;
    let site = map_of(db, f.site_a).await;
    // The same anchor (the same equipment, or another one installed at the same node): an ordinary
    // edit, even once proposed.
    let eq_a_bis = equipment(db, "A-BIS", Some(f.t.site_a)).await;
    propose(db, f.plan_a).await;
    let rv = row_version(db, f.plan_a).await;
    let moved = access::check_plan_update(db, &site, f.plan_a, rv, None, Some(f.eq_a))
        .await
        .expect("no target change");
    assert_eq!(moved, None);
    let moved = access::check_plan_update(db, &site, f.plan_a, rv, Some("equipment"), Some(eq_a_bis))
        .await
        .expect("other equipment at the same node");
    assert_eq!(moved, None);
    // Another node, even below the same site, is a move (the work-order rule compares nodes too):
    // only on a draft.
    let err = access::check_plan_update(db, &site, f.plan_a, rv, Some("equipment"), Some(f.eq_a1))
        .await
        .expect_err("proposed plan moved to line A1");
    assert_eq!(gate_of(&err), "GATE_PM_SITE_CHANGE_DRAFT_ONLY");
    let err = access::check_plan_update(db, &site, f.plan_b, row_version(db, f.plan_b).await, None, None)
        .await
        .expect_err("plan of site B");
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
}

#[tokio::test]
async fn moving_a_plan_to_another_site_needs_the_right_at_both_sites() {
    let f = fixture().await;
    let db = &f.db;
    let rv = row_version(db, f.plan_a).await;

    // Site A only: refused at site B.
    let site = map_of(db, f.site_a).await;
    let err = access::check_plan_update(db, &site, f.plan_a, rv, Some("equipment"), Some(f.eq_b))
        .await
        .expect_err("site A planner moving to site B");
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    // Nor can they pull a site-B plan into site A (refused at the old site).
    let err = access::check_plan_update(
        db,
        &site,
        f.plan_b,
        row_version(db, f.plan_b).await,
        Some("equipment"),
        Some(f.eq_a),
    )
    .await
    .expect_err("site A planner pulling a site B plan");
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");

    // Rights at both sites: allowed, and the move is described for the audit trail.
    let both = map_of(db, f.sites_ab).await;
    let moved = access::check_plan_update(db, &both, f.plan_a, rv, Some("equipment"), Some(f.eq_b))
        .await
        .expect("both sites")
        .expect("a site move");
    assert_eq!(
        moved,
        PlanSiteMove {
            old_scope_type: "equipment".into(),
            old_scope_id: Some(f.eq_a),
            old_anchor: Anchor::Node(f.t.site_a),
            new_scope_type: "equipment".into(),
            new_scope_id: Some(f.eq_b),
            new_anchor: Anchor::Node(f.t.site_b),
        }
    );
    // A location of site B is a move too.
    access::check_plan_update(db, &both, f.plan_a, rv, Some("location"), Some(f.t.site_b))
        .await
        .expect("both sites")
        .expect("a site move");
}

#[tokio::test]
async fn making_a_plan_plant_wide_needs_a_plant_wide_right() {
    let f = fixture().await;
    let db = &f.db;
    let rv = row_version(db, f.plan_a).await;
    for kind in ["family", "criticality_group"] {
        let both = map_of(db, f.sites_ab).await;
        let err = access::check_plan_update(db, &both, f.plan_a, rv, Some(kind), None)
            .await
            .expect_err(kind);
        assert_eq!(code_of(&err), "SCOPE_PLANT_WIDE_ONLY", "{kind}");
        let plant = map_of(db, f.plant).await;
        let moved = access::check_plan_update(db, &plant, f.plan_a, rv, Some(kind), None)
            .await
            .expect(kind)
            .expect("a site move");
        assert_eq!(moved.new_anchor, Anchor::Plant);
    }
    // And a plant-wide plan cannot be taken to a site by a site planner either.
    let site = map_of(db, f.site_a).await;
    let err = access::check_plan_update(
        db,
        &site,
        f.plan_family,
        row_version(db, f.plan_family).await,
        Some("equipment"),
        Some(f.eq_a),
    )
    .await
    .expect_err("family plan to site A");
    assert_eq!(code_of(&err), "SCOPE_PLANT_WIDE_ONLY");
}

#[tokio::test]
async fn a_plan_changes_site_only_while_it_is_a_draft() {
    let f = fixture().await;
    let db = &f.db;
    propose(db, f.plan_a).await;
    let rv = row_version(db, f.plan_a).await;
    // Refused to everyone, plant-wide planners included.
    for who in [f.plant, f.sites_ab] {
        let map = map_of(db, who).await;
        let err = access::check_plan_update(db, &map, f.plan_a, rv, Some("equipment"), Some(f.eq_b))
            .await
            .expect_err("proposed plan moved");
        assert_eq!(gate_of(&err), "GATE_PM_SITE_CHANGE_DRAFT_ONLY");
    }
    let plant = map_of(db, f.plant).await;
    let err = access::check_plan_update(db, &plant, f.plan_a, rv, Some("family"), None)
        .await
        .expect_err("proposed plan made plant-wide");
    assert_eq!(gate_of(&err), "GATE_PM_SITE_CHANGE_DRAFT_ONLY");
}

#[tokio::test]
async fn a_stale_row_version_is_refused_before_the_site_check_is_trusted() {
    let f = fixture().await;
    let db = &f.db;
    let plant = map_of(db, f.plant).await;
    let rv = row_version(db, f.plan_a).await;
    let err = access::check_plan_update(db, &plant, f.plan_a, rv - 1, Some("equipment"), Some(f.eq_b))
        .await
        .expect_err("stale");
    assert_eq!(gate_of(&err), "GATE_RECORD_CHANGED_ELSEWHERE");
}

#[tokio::test]
async fn a_site_move_is_recorded_with_the_update() {
    let f = fixture().await;
    let db = &f.db;
    let both = map_of(db, f.sites_ab).await;
    let rv = row_version(db, f.plan_a).await;
    let moved = access::check_plan_update(db, &both, f.plan_a, rv, Some("equipment"), Some(f.eq_b))
        .await
        .expect("both sites")
        .expect("a site move");
    let plan_ref = f.plan_a.to_string();
    let event = crate::audit::AuditEvent {
        event_type: access::AUDIT_ACTION_SITE_CHANGED,
        actor_id: Some(f.sites_ab),
        entity_type: Some("pm_plan"),
        entity_id: Some(plan_ref.as_str()),
        summary: "PM plan moved to another site",
        detail_json: Some(moved.details_json()),
        ..Default::default()
    };
    let updated = queries::update_pm_plan_recorded(
        db,
        f.plan_a,
        rv,
        UpdatePmPlanInput {
            asset_scope_type: Some("equipment".into()),
            asset_scope_id: Some(f.eq_b),
            ..no_change()
        },
        Some(event),
    )
    .await
    .expect("update");
    assert_eq!(updated.asset_scope_id, Some(f.eq_b));
    assert_eq!(updated.access_node_id, Some(f.t.site_b));
    let recorded = scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'pm.plan_site_changed' \
               AND target_type = 'pm_plan' AND target_id = '{}' AND actor_id = {} \
               AND details_json LIKE '%\"new_org_node_id\":{}%' AND details_json LIKE '%\"old_org_node_id\":{}%'",
            f.plan_a, f.sites_ab, f.t.site_b, f.t.site_a
        ),
    )
    .await;
    assert_eq!(recorded, 1);

    // A refused update records nothing (stale version: rolled back with it).
    let stale = crate::audit::AuditEvent {
        event_type: access::AUDIT_ACTION_SITE_CHANGED,
        entity_type: Some("pm_plan"),
        entity_id: Some(plan_ref.as_str()),
        summary: "PM plan moved to another site",
        ..Default::default()
    };
    queries::update_pm_plan_recorded(db, f.plan_a, rv, no_change(), Some(stale))
        .await
        .expect_err("stale");
    let recorded = scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'pm.plan_site_changed' AND target_id = '{}'",
            f.plan_a
        ),
    )
    .await;
    assert_eq!(recorded, 1);
}

#[tokio::test]
async fn rights_are_judged_as_held_now() {
    let f = fixture().await;
    let db = &f.db;
    let site = map_of(db, f.site_a).await;
    access::check_on_plan(db, &site, PM_EDIT, f.plan_a)
        .await
        .expect("held on site A");
    // The role on site A ends: the next check (fresh map) refuses.
    exec(
        db,
        &format!(
            "UPDATE user_scope_assignments SET valid_to = '2000-01-01T00:00:00Z' WHERE user_id = {}",
            f.site_a
        ),
    )
    .await;
    let now = map_of(db, f.site_a).await;
    let err = access::check_on_plan(db, &now, PM_EDIT, f.plan_a)
        .await
        .expect_err("role ended");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Reading (pm.view, CMMS Advisor decision 2026-11-06)
// ═══════════════════════════════════════════════════════════════════════════════

fn no_plan_filter() -> crate::pm::domain::PmPlanFilter {
    crate::pm::domain::PmPlanFilter {
        search: None,
        strategy_type: None,
        lifecycle_status: None,
        is_active: None,
    }
}

async fn listed_plans(db: &DatabaseConnection, user_id: i32) -> Vec<i64> {
    let scope = map_of(db, user_id).await.scope_of(PM_VIEW);
    queries::list_pm_plans_scoped(db, no_plan_filter(), &scope)
        .await
        .expect("list")
        .into_iter()
        .map(|p| p.id)
        .collect()
}

/// A forecast occurrence of `plan_id`, overdue (due in 2020).
async fn occurrence(db: &DatabaseConnection, key: &str, plan_id: i64, version_id: i64) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO pm_occurrences (entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, status)
             VALUES ('pm-scope-occ-{key}', {plan_id}, {version_id}, 'scope-{key}', '2020-01-01T00:00:00Z', 'forecasted')"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM pm_occurrences WHERE entity_sync_id = 'pm-scope-occ-{key}'"),
    )
    .await
}

#[tokio::test]
async fn a_site_reader_lists_their_sites_plans_and_every_family_or_group_plan() {
    let f = fixture().await;
    let db = &f.db;
    let site = listed_plans(db, f.site_a).await;
    for (plan_id, what) in [
        (f.plan_a, "plan on site A equipment"),
        (f.plan_loc_a1, "plan on location A1"),
        (f.plan_family, "family plan"),
        (f.plan_group, "criticality-group plan"),
    ] {
        assert!(site.contains(&plan_id), "{what} listed");
    }
    assert!(!site.contains(&f.plan_b), "plan of site B left out");

    // A plan on equipment installed nowhere: plant-wide readers only.
    let plan_none = plan(db, "PM-SCOPE-NONE", "equipment", Some(f.eq_none)).await;
    let site = listed_plans(db, f.site_a).await;
    assert!(!site.contains(&plan_none), "plan without a site left out");

    let plant = listed_plans(db, f.plant).await;
    for plan_id in [
        f.plan_a,
        f.plan_b,
        f.plan_loc_a1,
        f.plan_family,
        f.plan_group,
        plan_none,
    ] {
        assert!(plant.contains(&plan_id), "plant-wide reader sees {plan_id}");
    }
    let everything = queries::list_pm_plans(db, no_plan_filter()).await.expect("list");
    assert_eq!(plant.len(), everything.len(), "plant-wide reader sees every plan");
}

#[tokio::test]
async fn a_site_reader_opens_their_plans_and_family_plans_but_not_another_sites() {
    let f = fixture().await;
    let db = &f.db;
    let site = map_of(db, f.site_a).await;
    for plan_id in [f.plan_a, f.plan_loc_a1, f.plan_family, f.plan_group] {
        access::check_view_plan(db, &site, plan_id)
            .await
            .unwrap_or_else(|e| panic!("open {plan_id}: {e:?}"));
    }
    let err = access::check_view_plan(db, &site, f.plan_b)
        .await
        .expect_err("plan of site B");
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    // The plan form's previews follow the same rule.
    let err = access::check_view_target(db, &site, "equipment", Some(f.eq_b))
        .await
        .expect_err("site B equipment meters");
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    access::check_view_target(db, &site, "family", Some(1))
        .await
        .expect("family preview");

    let plant = map_of(db, f.plant).await;
    for plan_id in [f.plan_a, f.plan_b, f.plan_family] {
        access::check_view_plan(db, &plant, plan_id)
            .await
            .unwrap_or_else(|e| panic!("plant-wide opens {plan_id}: {e:?}"));
    }
    let nobody = user(db, "pm_view_nobody").await;
    let none = map_of(db, nobody).await;
    let err = access::check_view_plan(db, &none, f.plan_family)
        .await
        .expect_err("no pm.view anywhere");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
}

#[tokio::test]
async fn a_family_plan_is_read_only_to_a_site_reader() {
    let f = fixture().await;
    let db = &f.db;
    let site = map_of(db, f.site_a).await;
    access::check_view_plan(db, &site, f.plan_family)
        .await
        .expect("visible");
    let err = access::check_on_plan(db, &site, PM_EDIT, f.plan_family)
        .await
        .expect_err("not editable");
    assert_eq!(code_of(&err), "SCOPE_PLANT_WIDE_ONLY");
}

#[tokio::test]
async fn occurrence_lists_and_due_figures_follow_the_readers_sites() {
    let f = fixture().await;
    let db = &f.db;
    let version_family = version(db, f.plan_family).await;
    let occ_a = occurrence(db, "A", f.plan_a, f.version_a).await;
    let occ_b = occurrence(db, "B", f.plan_b, f.version_b).await;
    let occ_family = occurrence(db, "FAMILY", f.plan_family, version_family).await;

    let site_scope = map_of(db, f.site_a).await.scope_of(PM_VIEW);
    let plant_scope = map_of(db, f.plant).await.scope_of(PM_VIEW);
    let filter = || crate::pm::domain::PmOccurrenceFilter {
        pm_plan_id: None,
        status: None,
        due_from: None,
        due_to: None,
        include_completed: Some(true),
    };
    let site: Vec<i64> = queries::list_pm_occurrences_scoped(db, filter(), &site_scope)
        .await
        .expect("site list")
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert!(site.contains(&occ_a) && site.contains(&occ_family), "{site:?}");
    assert!(!site.contains(&occ_b), "site B occurrence left out");
    let plant: Vec<i64> = queries::list_pm_occurrences_scoped(db, filter(), &plant_scope)
        .await
        .expect("plant list")
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert!(plant.contains(&occ_a) && plant.contains(&occ_b) && plant.contains(&occ_family));

    let site_figures = queries::get_pm_due_metrics_scoped(db, &site_scope).await.expect("site");
    let plant_figures = queries::get_pm_due_metrics_scoped(db, &plant_scope)
        .await
        .expect("plant");
    let all_figures = queries::get_pm_due_metrics(db).await.expect("all");
    assert_eq!(plant_figures.overdue_count, all_figures.overdue_count);
    assert!(site_figures.overdue_count >= 2, "A and the family plan are overdue");
    assert!(
        plant_figures.overdue_count > site_figures.overdue_count,
        "the site B occurrence is not counted for the site reader"
    );
}
