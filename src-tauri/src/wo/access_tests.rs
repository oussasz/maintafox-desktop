//! Site scope of work orders (step 6, WO): on the org tree of `auth::scope_tests` (plant → site
//! A → line A1, plant → site B), a plant-wide user is unchanged, a user with a role on site A
//! works on site A and below only, a work order of site B is refused as outside their sites, and
//! a work order without an org node is refused to them as without site — visible to plant-wide
//! users, counted (never silently dropped) in the site user's list.

use sea_orm::DatabaseConnection;

use crate::auth::scope::{self, AccessMap};
use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::errors::AppError;
use crate::rbac::permissions::{
    OT_ADMIN, OT_APPROVE, OT_CLOSE, OT_CREATE, OT_EDIT, OT_EMERGENCY_ASSIGN, OT_REOPEN, OT_VIEW,
};
use crate::wo::access::{self, WoChild};
use crate::wo::domain::WoCreateInput;
use crate::wo::queries::{self, WoListFilter};

/// Every work-order right of the screens.
const ALL_OT: &[&str] = &[
    OT_VIEW,
    OT_CREATE,
    OT_EDIT,
    OT_APPROVE,
    OT_CLOSE,
    OT_REOPEN,
    OT_ADMIN,
    OT_EMERGENCY_ASSIGN,
];

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// Plant-wide role with every OT right.
    plant: i32,
    /// Every OT right, on site A only.
    site: i32,
    /// OT view only, on site A.
    site_viewer: i32,
    /// No role at all.
    nobody: i32,
    /// Equipment installed on site A, line A1, site B, and not installed anywhere.
    eq_a: i64,
    eq_a1: i64,
    eq_b: i64,
    eq_none: i64,
    /// Work orders on site A, line A1, site B, and one without an org node (an old row).
    wo_a: i64,
    wo_a1: i64,
    wo_b: i64,
    wo_none: i64,
    /// Work orders already in the seeded database, outside site A: (with a node, without one).
    before: (i64, i64),
}

async fn equipment(db: &DatabaseConnection, code: &str, node: Option<i64>) -> i64 {
    let node_sql = node.map_or_else(|| "NULL".to_string(), |n| n.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('wo-scope-eq-{code}', 'WO-SCOPE-{code}', 'Equipment {code}', 'active_in_service', {node_sql},
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'wo-scope-eq-{code}'"),
    )
    .await
}

fn create_input(equipment_id: i64, entity_id: Option<i64>, creator: i32, title: &str) -> WoCreateInput {
    WoCreateInput {
        type_code: "corrective".into(),
        equipment_id: Some(equipment_id),
        location_id: None,
        source_di_id: None,
        source_inspection_anomaly_id: None,
        source_ram_ishikawa_diagram_id: None,
        source_ishikawa_flow_node_id: None,
        source_rca_cause_text: None,
        entity_id,
        planner_id: None,
        urgency_code: Some("normal".into()),
        subtype_code: None,
        title: title.to_string(),
        description: Some(format!("{title} (site scope test)")),
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: Some(2.0),
        creator_id: i64::from(creator),
        requires_permit: None,
    }
}

async fn create(db: &DatabaseConnection, equipment_id: i64, creator: i32, title: &str) -> i64 {
    queries::create_work_order(db, create_input(equipment_id, None, creator, title))
        .await
        .expect("create WO")
        .id
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let before = (
        scalar(
            &db,
            "SELECT COUNT(*) FROM work_orders WHERE entity_id IS NOT NULL AND entity_id > 0",
        )
        .await,
        scalar(
            &db,
            "SELECT COUNT(*) FROM work_orders WHERE entity_id IS NULL OR entity_id <= 0",
        )
        .await,
    );
    let t = tree(&db).await;
    let plant = user(&db, "wo_plant").await;
    let site = user(&db, "wo_site").await;
    let site_viewer = user(&db, "wo_site_viewer").await;
    let nobody = user(&db, "wo_nobody").await;
    let plant_role = role(&db, "WO scope plant", ALL_OT).await;
    let site_role = role(&db, "WO scope site A", ALL_OT).await;
    let viewer_role = role(&db, "WO scope viewer A", &[OT_VIEW]).await;
    assign(&db, plant, plant_role, "tenant", None).await;
    assign(&db, site, site_role, "org_node", Some(t.site_a)).await;
    assign(&db, site_viewer, viewer_role, "org_node", Some(t.site_a)).await;

    let eq_a = equipment(&db, "A", Some(t.site_a)).await;
    let eq_a1 = equipment(&db, "A1", Some(t.line_a1)).await;
    let eq_b = equipment(&db, "B", Some(t.site_b)).await;
    let eq_none = equipment(&db, "NONE", None).await;
    let wo_a = create(&db, eq_a, site, "Leak on site A").await;
    let wo_a1 = create(&db, eq_a1, site, "Noise on line A1").await;
    let wo_b = create(&db, eq_b, plant, "Leak on site B").await;
    let wo_none = create(&db, eq_a, plant, "Old work order without site").await;
    // A row from before the node was required: stored without an org node.
    exec(
        &db,
        &format!("UPDATE work_orders SET entity_id = NULL WHERE id = {wo_none}"),
    )
    .await;
    Fixture {
        db,
        t,
        plant,
        site,
        site_viewer,
        nobody,
        eq_a,
        eq_a1,
        eq_b,
        eq_none,
        wo_a,
        wo_a1,
        wo_b,
        wo_none,
        before,
    }
}

/// The user's rights now (a fresh cache: roles change within a test).
async fn map_of(db: &DatabaseConnection, user_id: i32) -> AccessMap {
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    scope::access_map_cached(db, &cache, user_id).await.expect("access map")
}

fn all_filter() -> WoListFilter {
    WoListFilter {
        limit: 200,
        offset: 0,
        ..WoListFilter::default()
    }
}

fn ids(page: &queries::WoListPage) -> Vec<i64> {
    let mut ids: Vec<i64> = page.items.iter().map(|w| w.id).collect();
    ids.sort_unstable();
    ids
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

fn is_not_found(err: &AppError) -> bool {
    matches!(err, AppError::NotFound { .. })
}

// ═══════════════════════════════════════════════════════════════════════════════
// List, statistics, backlog, audit log
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_site_user_lists_their_sites_only_and_is_told_how_many_are_hidden_and_why() {
    let f = fixture().await;
    let site_scope = scope::access_scope(&f.db, f.site, OT_VIEW).await.expect("scope");
    let plant_scope = scope::access_scope(&f.db, f.plant, OT_VIEW).await.expect("scope");
    let (before_with_site, before_without_site) = f.before;

    let plant_page = queries::list_work_orders_in_scope(&f.db, all_filter(), &plant_scope)
        .await
        .expect("plant list");
    for id in [f.wo_a, f.wo_a1, f.wo_b, f.wo_none] {
        assert!(
            ids(&plant_page).contains(&id),
            "plant-wide: every work order, the one without site included"
        );
    }
    assert_eq!(plant_page.total, before_with_site + before_without_site + 4);
    assert_eq!(
        (plant_page.hidden_outside_scope, plant_page.hidden_without_site),
        (0, 0)
    );

    let site_page = queries::list_work_orders_in_scope(&f.db, all_filter(), &site_scope)
        .await
        .expect("site list");
    assert_eq!(
        ids(&site_page),
        sorted(vec![f.wo_a, f.wo_a1]),
        "site A and the line below it"
    );
    assert_eq!(site_page.total, 2, "the total counts the visible work orders");
    let expected_hidden = (before_with_site + 1, before_without_site + 1);
    assert_eq!(
        (site_page.hidden_outside_scope, site_page.hidden_without_site),
        expected_hidden,
        "site B's work order and the one without site, counted"
    );

    // Filtered in SQL before paging: one row per page, the total and the counts stay right.
    let first = queries::list_work_orders_in_scope(
        &f.db,
        WoListFilter {
            limit: 1,
            offset: 0,
            ..WoListFilter::default()
        },
        &site_scope,
    )
    .await
    .expect("page 1");
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.total, 2);
    assert_eq!((first.hidden_outside_scope, first.hidden_without_site), expected_hidden);

    // The hidden counts ignore the user's filters: a search matching site B's title (or
    // anything else) must not tell a site-A user whether it matched.
    for search in ["site B", "no work order says this"] {
        let searched = queries::list_work_orders_in_scope(
            &f.db,
            WoListFilter {
                search: Some(search.into()),
                ..all_filter()
            },
            &site_scope,
        )
        .await
        .expect("search");
        assert!(searched.items.is_empty());
        assert_eq!(
            (searched.hidden_outside_scope, searched.hidden_without_site),
            expected_hidden,
            "search {search:?} does not change the hidden counts"
        );
    }
    // Nor does an explicit site filter on another site.
    let other_site = queries::list_work_orders_in_scope(
        &f.db,
        WoListFilter {
            entity_id: Some(f.t.site_b),
            ..all_filter()
        },
        &site_scope,
    )
    .await
    .expect("site filter");
    assert!(other_site.items.is_empty(), "the filter is intersected with the scope");
    assert_eq!(
        (other_site.hidden_outside_scope, other_site.hidden_without_site),
        expected_hidden
    );

    // A role held nowhere lists nothing.
    let nowhere = scope::access_scope(&f.db, f.nobody, OT_VIEW).await.expect("scope");
    let empty = queries::list_work_orders_in_scope(&f.db, all_filter(), &nowhere)
        .await
        .expect("nowhere");
    assert_eq!(empty.total, 0);

    // The unscoped entry point (home, reports: later stage) is plant-wide as before.
    let unscoped = queries::list_work_orders(&f.db, all_filter()).await.expect("unscoped");
    assert_eq!(unscoped.total, plant_page.total);
    assert_eq!((unscoped.hidden_outside_scope, unscoped.hidden_without_site), (0, 0));
}

#[tokio::test]
async fn statistics_backlog_and_the_audit_log_cover_the_users_sites_only() {
    let f = fixture().await;
    let site_scope = scope::access_scope(&f.db, f.site, OT_VIEW).await.expect("scope");
    let plant_scope = scope::access_scope(&f.db, f.plant, OT_VIEW).await.expect("scope");
    let (before_with_site, before_without_site) = f.before;

    let site_stats = crate::wo::stats::get_wo_stats_in_scope(&f.db, &access::ScopeSql::of(&site_scope))
        .await
        .expect("site stats");
    assert_eq!(site_stats.total, 2, "site A and line A1 only");
    let by_status_total: i64 = site_stats.by_status.iter().map(|s| s.count).sum();
    assert_eq!(by_status_total, 2);
    let by_urgency_total: i64 = site_stats.by_urgency.iter().map(|s| s.count).sum();
    assert_eq!(by_urgency_total, 2);
    let mut entities: Vec<String> = site_stats.by_entity.iter().map(|e| e.entity.clone()).collect();
    entities.sort();
    assert_eq!(entities, vec!["Node A".to_string(), "Node A1".to_string()]);

    let plant_stats = crate::wo::stats::get_wo_stats_in_scope(&f.db, &access::ScopeSql::of(&plant_scope))
        .await
        .expect("plant stats");
    assert_eq!(plant_stats.total, before_with_site + before_without_site + 4);
    let unscoped = crate::wo::stats::get_wo_stats(&f.db).await.expect("unscoped stats");
    assert_eq!(unscoped.total, plant_stats.total, "plant-wide figures unchanged");

    // Backlog: the fixture's work orders are drafts without a planned start (unscheduled).
    let input = || crate::wo::backlog::WoBacklogHeatmapInput {
        start_date: None,
        weeks: 1,
    };
    let site_backlog = crate::wo::backlog::get_wo_backlog_heatmap_in_scope(&f.db, input(), &site_scope)
        .await
        .expect("site backlog");
    assert_eq!(site_backlog.unscheduled.wo_count, 2, "site A's drafts only");
    let plant_backlog = crate::wo::backlog::get_wo_backlog_heatmap_in_scope(&f.db, input(), &plant_scope)
        .await
        .expect("plant backlog");
    let unscoped_backlog = crate::wo::backlog::get_wo_backlog_heatmap(&f.db, input())
        .await
        .expect("unscoped backlog");
    assert!(plant_backlog.unscheduled.wo_count >= 4);
    assert_eq!(plant_backlog.unscheduled, unscoped_backlog.unscheduled);

    // Audit log: events of site A's work order only; an event without a work order has no site.
    for wo_id in [Some(f.wo_a), Some(f.wo_b), Some(f.wo_none), None] {
        crate::wo::audit::record_wo_change_event(
            &f.db,
            crate::wo::audit::WoAuditInput {
                wo_id,
                action: "site_scope_test".into(),
                actor_id: Some(i64::from(f.plant)),
                summary: None,
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;
    }
    let filter = || crate::wo::audit::WoAuditFilter {
        action: Some("site_scope_test".into()),
        actor_id: None,
        date_from: None,
        date_to: None,
        wo_id: None,
        limit: Some(50),
        offset: Some(0),
    };
    let admin_site = scope::access_scope(&f.db, f.site, OT_ADMIN).await.expect("scope");
    let site_events =
        crate::wo::audit::list_all_wo_change_events_in_scope(&f.db, filter(), &access::ScopeSql::of(&admin_site))
            .await
            .expect("site events");
    let event_wos: Vec<Option<i64>> = site_events.iter().map(|e| e.wo_id).collect();
    assert_eq!(event_wos, vec![Some(f.wo_a)]);
    let plant_events = crate::wo::audit::list_all_wo_change_events(&f.db, filter())
        .await
        .expect("plant events");
    assert_eq!(plant_events.len(), 4, "plant-wide: every event, as before");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Open, create, act
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn opening_a_work_order_is_checked_at_its_site_with_the_refusals_in_order() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let nobody = map_of(&f.db, f.nobody).await;

    access::check_on_wo(&f.db, &site, OT_VIEW, f.wo_a)
        .await
        .expect("own site");
    access::check_on_wo(&f.db, &site, OT_VIEW, f.wo_a1)
        .await
        .expect("line below the site");
    let err = access::check_on_wo(&f.db, &site, OT_VIEW, f.wo_b).await.unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_wo(&f.db, &site, OT_VIEW, f.wo_none).await.unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");

    for wo in [f.wo_a, f.wo_a1, f.wo_b, f.wo_none] {
        access::check_on_wo(&f.db, &plant, OT_VIEW, wo)
            .await
            .expect("plant-wide: everywhere");
    }

    // Order: the right held nowhere first, then not found, then the site.
    let missing = 9_999_999;
    let err = access::check_on_wo(&f.db, &nobody, OT_VIEW, missing).await.unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    let err = access::check_on_wo(&f.db, &nobody, OT_VIEW, f.wo_b).await.unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    let err = access::check_on_wo(&f.db, &site, OT_VIEW, missing).await.unwrap_err();
    assert!(is_not_found(&err), "{err:?}");

    // The reads of what belongs to a work order: a work order that does not exist has no site.
    let err = access::check_on_wo_records(&f.db, &site, OT_VIEW, missing)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    access::check_on_wo_records(&f.db, &plant, OT_VIEW, missing)
        .await
        .expect("plant-wide reads what is left, as before");
    let err = access::check_on_wo_records(&f.db, &site, OT_VIEW, f.wo_b)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
}

#[tokio::test]
async fn creating_a_work_order_needs_the_right_at_its_site_and_at_its_equipments_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let nobody = map_of(&f.db, f.nobody).await;
    let viewer = map_of(&f.db, f.site_viewer).await;

    // Own site: the node derived from the equipment, or given and matching.
    access::check_create_at(&f.db, &site, None, Some(f.eq_a))
        .await
        .expect("equipment on site A");
    access::check_create_at(&f.db, &site, None, Some(f.eq_a1))
        .await
        .expect("equipment on line A1");
    access::check_create_at(&f.db, &site, Some(f.t.site_a), Some(f.eq_a1))
        .await
        .expect("site A, equipment below it");

    // Another site: by its equipment, by the node given, or by an equipment of another site.
    let err = access::check_create_at(&f.db, &site, None, Some(f.eq_b))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_create_at(&f.db, &site, Some(f.t.site_b), Some(f.eq_a))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_create_at(&f.db, &site, Some(f.t.site_a), Some(f.eq_b))
        .await
        .unwrap_err();
    assert_eq!(
        code_of(&err),
        "SCOPE_OUTSIDE_SITE",
        "equipment of another site refused even under the user's site"
    );

    // No derivable node: plant-wide users only (the create then refuses it for everyone).
    let err = access::check_create_at(&f.db, &site, None, Some(f.eq_none))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    access::check_create_at(&f.db, &plant, None, Some(f.eq_none))
        .await
        .expect("plant-wide passes the scope check");
    let refused = queries::create_work_order(&f.db, create_input(f.eq_none, None, f.plant, "No site"))
        .await
        .unwrap_err();
    assert!(matches!(refused, AppError::ValidationFailed(_)), "{refused:?}");

    // Plant-wide: everywhere, unchanged.
    for (node, eq) in [(None, f.eq_b), (Some(f.t.site_b), f.eq_a), (Some(f.t.plant), f.eq_b)] {
        access::check_create_at(&f.db, &plant, node, Some(eq))
            .await
            .expect("plant-wide");
    }

    // Without ot.create anywhere: the plain refusal, whatever the site.
    for map in [&nobody, &viewer] {
        let err = access::check_create_at(&f.db, map, None, Some(f.eq_a))
            .await
            .unwrap_err();
        assert_eq!(code_of(&err), "PERMISSION_DENIED");
    }

    // The create itself anchors the new work order at the equipment's node.
    let created = queries::create_work_order(&f.db, create_input(f.eq_a1, None, f.site, "New on A1"))
        .await
        .expect("create");
    assert_eq!(created.entity_id, Some(f.t.line_a1));
}

#[tokio::test]
async fn actions_and_child_records_are_checked_at_the_work_orders_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let viewer = map_of(&f.db, f.site_viewer).await;

    // Same permission as before, at the anchor: start/edit, approve, close, reopen, assign.
    for permission in [OT_EDIT, OT_APPROVE, OT_CLOSE, OT_REOPEN, OT_EMERGENCY_ASSIGN] {
        access::check_on_wo(&f.db, &site, permission, f.wo_a)
            .await
            .expect("own site");
        let err = access::check_on_wo(&f.db, &site, permission, f.wo_b).await.unwrap_err();
        assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE", "{permission} on site B");
        let err = access::check_on_wo(&f.db, &site, permission, f.wo_none)
            .await
            .unwrap_err();
        assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE", "{permission} without site");
        access::check_on_wo(&f.db, &plant, permission, f.wo_none)
            .await
            .expect("plant-wide acts on the work order without site");
    }
    // A site viewer holds ot.view on site A, not ot.edit: the plain refusal, even on site A.
    let err = access::check_on_wo(&f.db, &viewer, OT_EDIT, f.wo_a).await.unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");

    // A child record addressed by its own id: checked at its work order's site.
    let task = |wo_id: i64| crate::wo::tasks::AddTaskInput {
        wo_id,
        task_description: "Check the coupling".into(),
        sequence_order: 1,
        is_mandatory: false,
        estimated_minutes: None,
        origin: None,
    };
    let task_a = crate::wo::tasks::add_task(&f.db, task(f.wo_a))
        .await
        .expect("task A")
        .id;
    let task_b = crate::wo::tasks::add_task(&f.db, task(f.wo_b))
        .await
        .expect("task B")
        .id;
    let task_none = crate::wo::tasks::add_task(&f.db, task(f.wo_none))
        .await
        .expect("task without site")
        .id;
    access::check_on_child(&f.db, &site, OT_EDIT, WoChild::Task, task_a)
        .await
        .expect("task of site A");
    let err = access::check_on_child(&f.db, &site, OT_EDIT, WoChild::Task, task_b)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_child(&f.db, &site, OT_EDIT, WoChild::Task, task_none)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    for id in [task_a, task_b, task_none] {
        access::check_on_child(&f.db, &plant, OT_EDIT, WoChild::Task, id)
            .await
            .expect("plant-wide");
    }
    let err = access::check_on_child(&f.db, &site, OT_EDIT, WoChild::Task, 9_999_999)
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");
    // A child of a kind addressed by id elsewhere (attachment): not found, not a site answer.
    let err = access::check_on_child(&f.db, &site, OT_ADMIN, WoChild::Attachment, 9_999_999)
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");

    // Moving a work order to other equipment (draft edit): the old and the new site (rule §2.7).
    access::check_equipment_change(&f.db, &site, OT_EDIT, f.wo_a, Some(f.eq_a1))
        .await
        .expect("to equipment of the same site");
    access::check_equipment_change(&f.db, &site, OT_EDIT, f.wo_a, Some(f.eq_a))
        .await
        .expect("equipment unchanged");
    access::check_equipment_change(&f.db, &site, OT_EDIT, f.wo_a, None)
        .await
        .expect("no equipment change");
    let err = access::check_equipment_change(&f.db, &site, OT_EDIT, f.wo_a, Some(f.eq_b))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE", "to equipment of site B");
    let err = access::check_equipment_change(&f.db, &site, OT_EDIT, f.wo_a, Some(f.eq_none))
        .await
        .unwrap_err();
    assert_eq!(
        code_of(&err),
        "SCOPE_RECORD_WITHOUT_SITE",
        "to equipment installed nowhere"
    );
    let err = access::check_equipment_change(&f.db, &site, OT_EDIT, f.wo_b, Some(f.eq_a))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE", "from a work order of site B");
    access::check_equipment_change(&f.db, &plant, OT_EDIT, f.wo_a, Some(f.eq_b))
        .await
        .expect("plant-wide");
}

fn draft_equipment_change(id: i64, row_version: i64, equipment_id: i64) -> crate::wo::domain::WoDraftUpdateInput {
    crate::wo::domain::WoDraftUpdateInput {
        id,
        expected_row_version: row_version,
        title: None,
        type_code: None,
        equipment_id: Some(equipment_id),
        location_id: None,
        description: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        notes: None,
        urgency_code: None,
        subtype_code: None,
        requires_permit: None,
    }
}

#[tokio::test]
async fn a_draft_moves_to_the_site_of_its_new_equipment() {
    let f = fixture().await;
    let before = queries::get_work_order(&f.db, f.wo_a)
        .await
        .expect("read")
        .expect("wo_a");
    assert_eq!(before.entity_id, Some(f.t.site_a));

    // Other equipment of the same site's line: the order now belongs to the line.
    let moved = queries::update_wo_draft_fields(&f.db, draft_equipment_change(f.wo_a, before.row_version, f.eq_a1))
        .await
        .expect("to equipment of line A1");
    assert_eq!(moved.entity_id, Some(f.t.line_a1));

    // Equipment installed nowhere: refused, as at create; the order is unchanged.
    let err = queries::update_wo_draft_fields(&f.db, draft_equipment_change(f.wo_a, moved.row_version, f.eq_none))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ValidationFailed(_)), "{err:?}");
    let kept = queries::get_work_order(&f.db, f.wo_a)
        .await
        .expect("read")
        .expect("wo_a");
    assert_eq!((kept.equipment_id, kept.entity_id), (Some(f.eq_a1), Some(f.t.line_a1)));

    // Equipment of site B (a plant-wide user may do it): the order moves to site B, so a site-A
    // user no longer sees it.
    let to_b = queries::update_wo_draft_fields(&f.db, draft_equipment_change(f.wo_a, kept.row_version, f.eq_b))
        .await
        .expect("to equipment of site B");
    assert_eq!(to_b.entity_id, Some(f.t.site_b));
    let site = map_of(&f.db, f.site).await;
    let err = access::check_on_wo(&f.db, &site, OT_VIEW, f.wo_a).await.unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE", "the site-A user no longer sees it");

    // A draft with a permit still open on it does not move to another site (decision of the CMMS
    // Advisor): the user closes or cancels the permit first. A closed one does not hold it back.
    exec(
        &f.db,
        &format!(
            "INSERT INTO work_permits (entity_sync_id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, status, row_version)
             SELECT 'site-move-permit', 'PTW-SITE-MOVE', wo.id, pt.id, wo.equipment_id, wo.entity_id, 'draft', 1
               FROM work_orders wo, permit_types pt WHERE wo.id = {} AND pt.code = 'hot_work'",
            f.wo_a
        ),
    )
    .await;
    let err = queries::update_wo_draft_fields(&f.db, draft_equipment_change(f.wo_a, to_b.row_version, f.eq_a))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::ValidationFailed(m) if m.iter().any(|e| e.contains("permis"))),
        "{err:?}"
    );
    let kept = queries::get_work_order(&f.db, f.wo_a)
        .await
        .expect("read")
        .expect("wo_a");
    assert_eq!((kept.equipment_id, kept.entity_id), (Some(f.eq_b), Some(f.t.site_b)));
    exec(
        &f.db,
        "UPDATE work_permits SET status = 'closed' WHERE entity_sync_id = 'site-move-permit'",
    )
    .await;
    let back = queries::update_wo_draft_fields(&f.db, draft_equipment_change(f.wo_a, kept.row_version, f.eq_a))
        .await
        .expect("back to site A once the permit is closed");
    assert_eq!(back.entity_id, Some(f.t.site_a));
}
