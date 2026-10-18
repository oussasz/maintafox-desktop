//! Site scope of intervention requests (step 6, DI): on the org tree of `auth::scope_tests`
//! (plant → site A → line A1, plant → site B), a plant-wide user is unchanged, a user with a role
//! on site A works on site A and below only, a request of site B is refused as outside their
//! sites, and a request without an org node (old `0` rows) is refused to them as without site —
//! visible to plant-wide users, counted (never silently dropped) in the site user's list.

use sea_orm::DatabaseConnection;

use crate::auth::scope::{self, AccessMap};
use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::di::access;
use crate::di::queries::{self, DiCreateInput, DiListFilter};
use crate::errors::AppError;
use crate::rbac::permissions::{
    DI_ADMIN, DI_APPROVE, DI_CONVERT, DI_CREATE, DI_CREATE_OWN, DI_REVIEW, DI_SCREEN, DI_VIEW, OT_CREATE,
};

/// Every DI right of the screens, plus the work-order create right the conversion needs.
const ALL_DI: &[&str] = &[
    DI_VIEW,
    DI_CREATE,
    DI_CREATE_OWN,
    DI_SCREEN,
    DI_REVIEW,
    DI_APPROVE,
    DI_CONVERT,
    DI_ADMIN,
    OT_CREATE,
];

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// Plant-wide role with every DI right.
    plant: i32,
    /// Every DI right, on site A only.
    site: i32,
    /// Equipment installed on site A, line A1, site B.
    eq_a: i64,
    eq_b: i64,
    /// Requests on site A, line A1, site B, and one without an org node (an old `0` row).
    di_a: i64,
    di_a1: i64,
    di_b: i64,
    di_none: i64,
}

async fn equipment(db: &DatabaseConnection, code: &str, node: i64) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('di-scope-eq-{code}', 'DI-SCOPE-{code}', 'Equipment {code}', 'active_in_service', {node},
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'di-scope-eq-{code}'"),
    )
    .await
}

fn create_input(equipment_id: i64, org_node_id: i64, submitter_id: i32, symptom: i64, title: &str) -> DiCreateInput {
    DiCreateInput {
        equipment_id,
        org_node_id,
        title: title.into(),
        description: format!("{title} (site scope test)"),
        origin_type: "operator".into(),
        request_type: "repair".to_string(),
        symptom_code_id: Some(symptom),
        impact_level: "unknown".into(),
        production_impact: false,
        safety_flag: false,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: "normal".into(),
        observed_at: None,
        submitter_id: i64::from(submitter_id),
        source_inspection_anomaly_id: None,
    }
}

async fn symptom(db: &DatabaseConnection) -> i64 {
    crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("lookup")
        .expect("seeded symptom")
}

async fn create(db: &DatabaseConnection, equipment_id: i64, node: i64, submitter: i32, title: &str) -> i64 {
    let s = symptom(db).await;
    queries::create_intervention_request(db, create_input(equipment_id, node, submitter, s, title))
        .await
        .expect("create DI")
        .id
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let plant = user(&db, "di_plant").await;
    let site = user(&db, "di_site").await;
    let plant_role = role(&db, "DI scope plant", ALL_DI).await;
    let site_role = role(&db, "DI scope site A", ALL_DI).await;
    assign(&db, plant, plant_role, "tenant", None).await;
    assign(&db, site, site_role, "org_node", Some(t.site_a)).await;

    let eq_a = equipment(&db, "A", t.site_a).await;
    let eq_a1 = equipment(&db, "A1", t.line_a1).await;
    let eq_b = equipment(&db, "B", t.site_b).await;
    let di_a = create(&db, eq_a, t.site_a, site, "Leak on site A").await;
    let di_a1 = create(&db, eq_a1, t.line_a1, site, "Noise on line A1").await;
    let di_b = create(&db, eq_b, t.site_b, plant, "Leak on site B").await;
    let di_none = create(&db, eq_a, t.site_a, plant, "Old request without site").await;
    // A row from before decision O7: stored with org node 0 (not rewritten by this change).
    exec(
        &db,
        &format!("UPDATE intervention_requests SET org_node_id = 0 WHERE id = {di_none}"),
    )
    .await;
    Fixture {
        db,
        t,
        plant,
        site,
        eq_a,
        eq_b,
        di_a,
        di_a1,
        di_b,
        di_none,
    }
}

/// The user's rights now (a fresh cache: roles change within a test).
async fn map_of(db: &DatabaseConnection, user_id: i32) -> AccessMap {
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    scope::access_map_cached(db, &cache, user_id).await.expect("access map")
}

fn all_filter() -> DiListFilter {
    DiListFilter {
        limit: 200,
        offset: 0,
        ..DiListFilter::default()
    }
}

fn ids(page: &queries::DiListPage) -> Vec<i64> {
    let mut ids: Vec<i64> = page.items.iter().map(|d| d.id).collect();
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
// Lists, counts, statistics, audit log
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_site_user_lists_their_sites_only_and_is_told_how_many_are_hidden_and_why() {
    let f = fixture().await;
    let site_scope = scope::access_scope(&f.db, f.site, DI_VIEW).await.expect("scope");
    let plant_scope = scope::access_scope(&f.db, f.plant, DI_VIEW).await.expect("scope");

    let plant_page = queries::list_intervention_requests_in_scope(&f.db, all_filter(), &plant_scope)
        .await
        .expect("plant list");
    assert_eq!(
        ids(&plant_page),
        sorted(vec![f.di_a, f.di_a1, f.di_b, f.di_none]),
        "plant-wide: every request, the one without site included"
    );
    assert_eq!(plant_page.total, 4);
    assert_eq!(
        (plant_page.hidden_outside_scope, plant_page.hidden_without_site),
        (0, 0)
    );

    let site_page = queries::list_intervention_requests_in_scope(&f.db, all_filter(), &site_scope)
        .await
        .expect("site list");
    assert_eq!(
        ids(&site_page),
        sorted(vec![f.di_a, f.di_a1]),
        "site A and the line below it"
    );
    assert_eq!(site_page.total, 2, "the total counts the visible requests");
    assert_eq!(site_page.hidden_outside_scope, 1, "site B's request, counted");
    assert_eq!(site_page.hidden_without_site, 1, "the request without site, counted");

    // Filtered in SQL before paging: one row per page, the total and the counts stay right.
    let first = queries::list_intervention_requests_in_scope(
        &f.db,
        DiListFilter {
            limit: 1,
            offset: 0,
            ..DiListFilter::default()
        },
        &site_scope,
    )
    .await
    .expect("page 1");
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.total, 2);
    assert_eq!((first.hidden_outside_scope, first.hidden_without_site), (1, 1));

    // The hidden counts follow the user's filters: searching site B's title hides it only.
    let searched = queries::list_intervention_requests_in_scope(
        &f.db,
        DiListFilter {
            search: Some("site B".into()),
            ..all_filter()
        },
        &site_scope,
    )
    .await
    .expect("search");
    assert!(searched.items.is_empty());
    assert_eq!((searched.hidden_outside_scope, searched.hidden_without_site), (1, 0));

    // The unscoped entry point (home, dashboard: later stage) is plant-wide as before.
    let unscoped = queries::list_intervention_requests(&f.db, all_filter())
        .await
        .expect("unscoped");
    assert_eq!(unscoped.total, 4);
}

#[tokio::test]
async fn queue_badges_statistics_and_the_audit_log_cover_the_users_sites_only() {
    let f = fixture().await;
    let site = access::ScopeSql::of(&scope::access_scope(&f.db, f.site, DI_VIEW).await.expect("scope"));
    let plant = access::ScopeSql::of(&scope::access_scope(&f.db, f.plant, DI_VIEW).await.expect("scope"));

    let site_counts = queries::count_di_queues_in_scope(&f.db, i64::from(f.site), &site)
        .await
        .expect("site counts");
    let plant_counts = queries::count_di_queues_in_scope(&f.db, i64::from(f.plant), &plant)
        .await
        .expect("plant counts");
    assert_eq!(site_counts.to_screen, 2, "the two submitted requests of site A");
    assert_eq!(plant_counts.to_screen, 4);
    assert_eq!(site_counts.mine, 2, "their own requests, on their site");

    let filter = || crate::di::stats::DiStatsFilter {
        date_from: None,
        date_to: None,
        entity_id: None,
    };
    let site_stats = crate::di::stats::get_di_stats_in_scope(&f.db, filter(), &site)
        .await
        .expect("site stats");
    let plant_stats = crate::di::stats::get_di_stats_in_scope(&f.db, filter(), &plant)
        .await
        .expect("plant stats");
    assert_eq!(site_stats.total, 2);
    assert_eq!(plant_stats.total, 4);
    assert_eq!(
        site_stats.backlog_by_status.iter().map(|b| b.count).sum::<i64>(),
        2,
        "open backlog of site A only"
    );
    // An explicit site filter is intersected with the scope: site B gives nothing to a site-A user.
    let site_b_stats = crate::di::stats::get_di_stats_in_scope(
        &f.db,
        crate::di::stats::DiStatsFilter {
            entity_id: Some(f.t.site_b),
            ..filter()
        },
        &site,
    )
    .await
    .expect("site B for a site A user");
    assert_eq!(site_b_stats.total, 0);

    for di in [f.di_a, f.di_b, f.di_none] {
        crate::di::audit::record_di_change_event(
            &f.db,
            crate::di::audit::DiAuditInput {
                di_id: Some(di),
                action: "screened".into(),
                actor_id: Some(i64::from(f.plant)),
                summary: None,
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;
    }
    let audit_filter = || crate::di::audit::DiAuditFilter {
        action: Some("screened".into()),
        actor_id: None,
        date_from: None,
        date_to: None,
        limit: Some(100),
        offset: Some(0),
    };
    let admin_site = access::ScopeSql::of(&scope::access_scope(&f.db, f.site, DI_ADMIN).await.expect("scope"));
    let site_events = crate::di::audit::list_all_change_events(&f.db, audit_filter(), &admin_site)
        .await
        .expect("site audit");
    assert_eq!(
        site_events.iter().map(|e| e.di_id).collect::<Vec<_>>(),
        vec![Some(f.di_a)],
        "the audit log of site A's requests only"
    );
    let plant_events = crate::di::audit::list_all_change_events(&f.db, audit_filter(), &plant)
        .await
        .expect("plant audit");
    assert_eq!(plant_events.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// One request
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn opening_a_request_is_checked_at_its_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;

    for di in [f.di_a, f.di_a1, f.di_b, f.di_none] {
        access::check_on_di(&f.db, &plant, DI_VIEW, di)
            .await
            .expect("plant-wide reads every request");
    }
    access::check_on_di(&f.db, &site, DI_VIEW, f.di_a)
        .await
        .expect("own site");
    access::check_on_di(&f.db, &site, DI_VIEW, f.di_a1)
        .await
        .expect("a line below the site");
    let other = access::check_on_di(&f.db, &site, DI_VIEW, f.di_b)
        .await
        .expect_err("sibling site");
    assert_eq!(code_of(&other), "SCOPE_OUTSIDE_SITE");
    let none = access::check_on_di(&f.db, &site, DI_VIEW, f.di_none)
        .await
        .expect_err("no site");
    assert_eq!(code_of(&none), "SCOPE_RECORD_WITHOUT_SITE");

    // A missing request keeps its not-found answer; a user without the right anywhere keeps the
    // plain refusal (nothing learnt about which requests exist).
    let missing = access::check_on_di(&f.db, &site, DI_VIEW, 9_999_999)
        .await
        .expect_err("missing");
    assert!(is_not_found(&missing), "got {missing:?}");
    let nobody = map_of(&f.db, user(&f.db, "di_nobody").await).await;
    let refused = access::check_on_di(&f.db, &nobody, DI_VIEW, 9_999_999)
        .await
        .expect_err("no right");
    assert_eq!(code_of(&refused), "PERMISSION_DENIED");

    // Events / attachments of a request that no longer exists: plant-wide only, as before.
    access::check_on_di_records(&f.db, &plant, DI_VIEW, 9_999_999)
        .await
        .expect("plant-wide reads what is left");
    let gone = access::check_on_di_records(&f.db, &site, DI_VIEW, 9_999_999)
        .await
        .expect_err("site user");
    assert_eq!(code_of(&gone), "SCOPE_RECORD_WITHOUT_SITE");
    let events_b = access::check_on_di_records(&f.db, &site, DI_VIEW, f.di_b)
        .await
        .expect_err("events of a sibling site's request");
    assert_eq!(code_of(&events_b), "SCOPE_OUTSIDE_SITE");

    // The similar requests shown on a request: only those the reader may open.
    let site_scope = site.scope_of(DI_VIEW);
    let similar = queries::get_recent_similar_dis_in_scope(&f.db, f.eq_a, None, 30, &site_scope)
        .await
        .expect("similar");
    assert_eq!(similar.iter().map(|s| s.id).collect::<Vec<_>>(), vec![f.di_a]);
    let similar_plant = queries::get_recent_similar_dis(&f.db, f.eq_a, None, 30)
        .await
        .expect("similar plant");
    assert_eq!(similar_plant.len(), 2, "the request without site included");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Create (rule §2.6, decision O7)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn creating_a_request_needs_an_org_node_and_the_right_at_its_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;

    // O7: no org node, refused for everyone, plant-wide included, with its code.
    for map in [&site, &plant] {
        let err = access::check_create_at(&f.db, map, 0, f.eq_a)
            .await
            .expect_err("no org node");
        assert_eq!(code_of(&err), "DI_ORG_NODE_REQUIRED");
    }
    // Every path that creates a request refuses it too (inspection and PM follow-ups included).
    let s = symptom(&f.db).await;
    let err = queries::create_intervention_request(&f.db, create_input(f.eq_a, 0, f.plant, s, "No site"))
        .await
        .expect_err("domain refusal");
    assert_eq!(code_of(&err), "DI_ORG_NODE_REQUIRED");

    access::check_create_at(&f.db, &site, f.t.site_a, f.eq_a)
        .await
        .expect("own site");
    access::check_create_at(&f.db, &site, f.t.line_a1, f.eq_a)
        .await
        .expect("a line below the site, equipment of the site");
    let other = access::check_create_at(&f.db, &site, f.t.site_b, f.eq_b)
        .await
        .expect_err("sibling site");
    assert_eq!(code_of(&other), "SCOPE_OUTSIDE_SITE");
    let foreign_equipment = access::check_create_at(&f.db, &site, f.t.site_a, f.eq_b)
        .await
        .expect_err("equipment of another site");
    assert_eq!(code_of(&foreign_equipment), "SCOPE_OUTSIDE_SITE");
    access::check_create_at(&f.db, &plant, f.t.site_b, f.eq_b)
        .await
        .expect("plant-wide creates anywhere");

    // di.create.own alone is enough at the site (the "own or any" rule, unchanged).
    let own_only = user(&f.db, "di_own_only").await;
    let own_role = role(&f.db, "DI scope own only", &[DI_VIEW, DI_CREATE_OWN]).await;
    assign(&f.db, own_only, own_role, "org_node", Some(f.t.site_a)).await;
    let passed = access::check_create_at(&f.db, &map_of(&f.db, own_only).await, f.t.site_a, f.eq_a)
        .await
        .expect("own");
    assert_eq!(passed, vec![DI_CREATE_OWN]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Actions
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn decisions_on_a_request_are_checked_at_its_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let screen_or_review = [DI_SCREEN, DI_REVIEW];

    // Triage / screening: di.screen or di.review at the request's node.
    access::check_any_on_di(&f.db, &site, &screen_or_review, f.di_a, "denied")
        .await
        .expect("own site");
    let other = access::check_any_on_di(&f.db, &site, &screen_or_review, f.di_b, "denied")
        .await
        .expect_err("sibling site");
    assert_eq!(code_of(&other), "SCOPE_OUTSIDE_SITE");
    let none = access::check_any_on_di(&f.db, &site, &screen_or_review, f.di_none, "denied")
        .await
        .expect_err("no site");
    assert_eq!(code_of(&none), "SCOPE_RECORD_WITHOUT_SITE");
    for di in [f.di_a, f.di_b, f.di_none] {
        access::check_any_on_di(&f.db, &plant, &screen_or_review, di, "denied")
            .await
            .expect("plant-wide");
    }
    // Neither right anywhere: the command's own refusal, before looking the request up.
    let viewer = user(&f.db, "di_viewer").await;
    let viewer_role = role(&f.db, "DI scope viewer", &[DI_VIEW]).await;
    assign(&f.db, viewer, viewer_role, "tenant", None).await;
    let refused = access::check_any_on_di(&f.db, &map_of(&f.db, viewer).await, &screen_or_review, 9_999_999, "no")
        .await
        .expect_err("no right");
    assert!(
        matches!(refused, AppError::PermissionDenied(ref m) if m == "no"),
        "got {refused:?}"
    );

    // Approval (a single right) at the request's node.
    access::check_on_di(&f.db, &site, DI_APPROVE, f.di_a1)
        .await
        .expect("approve below the site");
    let approve_b = access::check_on_di(&f.db, &site, DI_APPROVE, f.di_b)
        .await
        .expect_err("approve on a sibling site");
    assert_eq!(code_of(&approve_b), "SCOPE_OUTSIDE_SITE");

    // Draft edit / attachment upload: the requester's own right or review, at the node.
    let own_only = user(&f.db, "di_requester").await;
    let own_role = role(&f.db, "DI scope requester", &[DI_VIEW, DI_CREATE_OWN]).await;
    assign(&f.db, own_only, own_role, "org_node", Some(f.t.site_a)).await;
    let mine_a = create(&f.db, f.eq_a, f.t.site_a, own_only, "My request on A").await;
    let mine_b = create(&f.db, f.eq_b, f.t.site_b, own_only, "My request on B").await;
    let requester = map_of(&f.db, own_only).await;
    let load = |id: i64| {
        let db = f.db.clone();
        async move {
            queries::get_intervention_request(&db, id)
                .await
                .expect("load")
                .expect("exists")
        }
    };
    assert_eq!(
        access::check_own_or_review(&f.db, &requester, own_only, &load(mine_a).await)
            .await
            .expect("own request on own site"),
        DI_CREATE_OWN
    );
    let mine_elsewhere = access::check_own_or_review(&f.db, &requester, own_only, &load(mine_b).await)
        .await
        .expect_err("own request on another site");
    assert_eq!(code_of(&mine_elsewhere), "SCOPE_OUTSIDE_SITE");
    let not_mine = access::check_own_or_review(&f.db, &requester, own_only, &load(f.di_a).await)
        .await
        .expect_err("someone else's request, no review right");
    assert_eq!(code_of(&not_mine), "PERMISSION_DENIED");
}

#[tokio::test]
async fn attachments_are_checked_at_their_requests_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let attach = |di: i64, name: &str| {
        let db = f.db.clone();
        let name = name.to_string();
        async move {
            exec(
                &db,
                &format!(
                    "INSERT INTO di_attachments (di_id, file_name, relative_path, mime_type, size_bytes,
                        attachment_type, uploaded_by_id, uploaded_at, notes)
                     VALUES ({di}, '{name}.jpg', 'di/{name}.jpg', 'image/jpeg', 10, 'photo', NULL,
                             datetime('now'), NULL)"
                ),
            )
            .await;
            scalar(
                &db,
                &format!("SELECT id FROM di_attachments WHERE relative_path = 'di/{name}.jpg'"),
            )
            .await
        }
    };
    let att_a = attach(f.di_a, "scope-a").await;
    let att_b = attach(f.di_b, "scope-b").await;
    let att_none = attach(f.di_none, "scope-none").await;

    // Delete (di.admin) and preview (di.view) of an attachment: at its request's node.
    for permission in [DI_ADMIN, DI_VIEW] {
        access::check_on_attachment(&f.db, &site, permission, att_a)
            .await
            .expect("own site");
        let other = access::check_on_attachment(&f.db, &site, permission, att_b)
            .await
            .expect_err("sibling site");
        assert_eq!(code_of(&other), "SCOPE_OUTSIDE_SITE");
        let none = access::check_on_attachment(&f.db, &site, permission, att_none)
            .await
            .expect_err("request without site");
        assert_eq!(code_of(&none), "SCOPE_RECORD_WITHOUT_SITE");
        for att in [att_a, att_b, att_none] {
            access::check_on_attachment(&f.db, &plant, permission, att)
                .await
                .expect("plant-wide");
        }
    }
    let missing = access::check_on_attachment(&f.db, &site, DI_ADMIN, 9_999_999)
        .await
        .expect_err("missing attachment");
    assert!(is_not_found(&missing), "got {missing:?}");
}

#[tokio::test]
async fn converting_needs_the_work_order_right_where_the_work_order_will_be() {
    let f = fixture().await;
    let plant = map_of(&f.db, f.plant).await;
    for di in [f.di_a, f.di_b, f.di_none] {
        access::check_convert(&f.db, &plant, di).await.expect("plant-wide");
    }
    let site = map_of(&f.db, f.site).await;
    assert_eq!(
        access::check_convert(&f.db, &site, f.di_a1).await.expect("own site"),
        scope::Anchor::Node(f.t.line_a1),
        "the work order is anchored at the request's node"
    );
    let other = access::check_convert(&f.db, &site, f.di_b)
        .await
        .expect_err("sibling site");
    assert_eq!(code_of(&other), "SCOPE_OUTSIDE_SITE");
    let none = access::check_convert(&f.db, &site, f.di_none)
        .await
        .expect_err("no site");
    assert_eq!(code_of(&none), "SCOPE_RECORD_WITHOUT_SITE");

    // di.convert without the work-order create right: refused.
    let converter = user(&f.db, "di_converter").await;
    let convert_only = role(&f.db, "DI scope convert only", &[DI_VIEW, DI_CONVERT]).await;
    assign(&f.db, converter, convert_only, "org_node", Some(f.t.site_a)).await;
    let refused = access::check_convert(&f.db, &map_of(&f.db, converter).await, f.di_a)
        .await
        .expect_err("no work-order right");
    assert_eq!(code_of(&refused), "PERMISSION_DENIED");
    // The work-order right held on the other site only: refused at this request's site.
    let wo_b = role(&f.db, "DI scope WO on B", &[OT_CREATE]).await;
    assign(&f.db, converter, wo_b, "org_node", Some(f.t.site_b)).await;
    let wrong_site = access::check_convert(&f.db, &map_of(&f.db, converter).await, f.di_a)
        .await
        .expect_err("work-order right on another site");
    assert_eq!(code_of(&wrong_site), "SCOPE_OUTSIDE_SITE");
}

/// Rule §2.7 (a move needs the right at the old and the new node) has nothing to check in the
/// DI commands today because no command moves a request: the draft edit carries neither the org
/// node nor the equipment. This locks it — if a field to move a request is ever added, this test
/// fails and the old-and-new check must come with it.
#[tokio::test]
async fn the_draft_edit_cannot_move_a_request_to_another_site() {
    let f = fixture().await;
    let current = queries::get_intervention_request(&f.db, f.di_a)
        .await
        .expect("load")
        .expect("exists");
    let input: queries::DiDraftUpdateInput = serde_json::from_value(serde_json::json!({
        "id": f.di_a,
        "expected_row_version": current.row_version,
        "title": "Edited",
        "org_node_id": f.t.site_b,
        "equipment_id": f.eq_b,
    }))
    .expect("draft input");
    let updated = queries::update_di_draft_fields(&f.db, input).await.expect("edit");
    assert_eq!(updated.title, "Edited");
    assert_eq!(updated.org_node_id, f.t.site_a, "the request stays on its site");
    assert_eq!(updated.equipment_id, f.eq_a);
}
