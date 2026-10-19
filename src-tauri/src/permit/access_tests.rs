//! Site scope of work permits (step 6, permits/LOTO): on the org tree of `auth::scope_tests`
//! (plant → site A → line A1, plant → site B), a plant-wide user is unchanged, a user with a role
//! on site A works on the permits of site A and below only, a permit of site B is refused as
//! outside their sites, and a permit without an org node is refused to them as without site —
//! visible to plant-wide users, counted (never silently dropped) in the site user's lists. The
//! people who may receive a hand-over hold the permit close right at the permit's site.

use sea_orm::DatabaseConnection;

use crate::auth::scope::{self, AccessMap, Anchor};
use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::errors::AppError;
use crate::permit::access::{self, PermitChild};
use crate::permit::domain::{
    PermitHandoverLogInput, WorkPermit, WorkPermitCreateInput, WorkPermitListFilter, WorkPermitListPage,
    WorkPermitStatusInput, WorkPermitUpdateInput,
};
use crate::permit::queries;
use crate::rbac::permissions::{PTW_CLOSE, PTW_ISSUE, PTW_VIEW};
use crate::wo::domain::WoCreateInput;

const ALL_PTW: &[&str] = &[PTW_VIEW, PTW_ISSUE, PTW_CLOSE];

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// Every permit right, plant-wide.
    plant: i32,
    /// Every permit right, on site A only.
    site: i32,
    /// The permit close right, on site B only.
    closer_b: i32,
    /// No role at all.
    nobody: i32,
    /// Equipment installed on site A, line A1, site B, and not installed anywhere.
    eq_a: i64,
    eq_a1: i64,
    eq_b: i64,
    eq_none: i64,
    /// Work orders on site A, site B, and one without an org node.
    wo_a: i64,
    wo_b: i64,
    wo_none: i64,
    /// Permits on site A, line A1, site B, and one without an org node (an old or received row).
    p_a: WorkPermit,
    p_a1: WorkPermit,
    p_b: WorkPermit,
    p_none: WorkPermit,
    /// Permits already in the seeded database: (with a node, without one).
    before: (i64, i64),
}

async fn equipment(db: &DatabaseConnection, code: &str, node: Option<i64>) -> i64 {
    let node_sql = node.map_or_else(|| "NULL".to_string(), |n| n.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('ptw-scope-eq-{code}', 'PTW-SCOPE-{code}', 'Equipment {code}', 'active_in_service', {node_sql},
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'ptw-scope-eq-{code}'"),
    )
    .await
}

async fn work_order(db: &DatabaseConnection, equipment_id: i64, creator: i32, title: &str) -> i64 {
    crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(equipment_id),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: title.to_string(),
            description: Some(format!("{title} (permit site scope test)")),
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(2.0),
            creator_id: i64::from(creator),
            requires_permit: None,
        },
    )
    .await
    .expect("create WO")
    .id
}

async fn hot_work(db: &DatabaseConnection) -> i64 {
    scalar(db, "SELECT id FROM permit_types WHERE code = 'hot_work'").await
}

fn create_input(type_id: i64, asset: Option<i64>, entity: Option<i64>, wo: Option<i64>) -> WorkPermitCreateInput {
    WorkPermitCreateInput {
        permit_type_id: type_id,
        asset_id: asset,
        entity_id: entity,
        linked_work_order_id: wo,
        expires_at: Some(super::tests::future_expiry()),
    }
}

async fn permit(db: &DatabaseConnection, asset: i64) -> WorkPermit {
    let type_id = hot_work(db).await;
    queries::create_work_permit(db, create_input(type_id, Some(asset), None, None))
        .await
        .expect("create permit")
}

async fn set_status(db: &DatabaseConnection, wp: &WorkPermit, status: &str) -> WorkPermit {
    queries::set_work_permit_status(
        db,
        WorkPermitStatusInput {
            id: wp.id,
            status: status.into(),
            expected_row_version: wp.row_version,
        },
        1,
    )
    .await
    .unwrap_or_else(|e| panic!("-> {status}: {e:?}"))
}

/// `draft` → … → `active`.
async fn activate(db: &DatabaseConnection, mut wp: WorkPermit) -> WorkPermit {
    for s in ["pending_review", "approved", "issued", "active"] {
        wp = set_status(db, &wp, s).await;
    }
    wp
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let before = (
        scalar(&db, "SELECT COUNT(*) FROM work_permits WHERE entity_id > 0").await,
        scalar(
            &db,
            "SELECT COUNT(*) FROM work_permits WHERE entity_id IS NULL OR entity_id <= 0",
        )
        .await,
    );
    let t = tree(&db).await;
    let plant = user(&db, "ptw_plant").await;
    let site = user(&db, "ptw_site").await;
    let closer_b = user(&db, "ptw_closer_b").await;
    let nobody = user(&db, "ptw_nobody").await;
    let plant_role = role(&db, "PTW scope plant", ALL_PTW).await;
    let site_role = role(&db, "PTW scope site A", ALL_PTW).await;
    let closer_role = role(&db, "PTW scope closer B", &[PTW_CLOSE]).await;
    assign(&db, plant, plant_role, "tenant", None).await;
    assign(&db, site, site_role, "org_node", Some(t.site_a)).await;
    assign(&db, closer_b, closer_role, "org_node", Some(t.site_b)).await;

    let eq_a = equipment(&db, "A", Some(t.site_a)).await;
    let eq_a1 = equipment(&db, "A1", Some(t.line_a1)).await;
    let eq_b = equipment(&db, "B", Some(t.site_b)).await;
    let eq_none = equipment(&db, "NONE", None).await;
    let wo_a = work_order(&db, eq_a, plant, "Leak on site A").await;
    let wo_b = work_order(&db, eq_b, plant, "Leak on site B").await;
    let wo_none = work_order(&db, eq_a, plant, "Old work order without site").await;
    exec(
        &db,
        &format!("UPDATE work_orders SET entity_id = NULL WHERE id = {wo_none}"),
    )
    .await;

    let p_a = permit(&db, eq_a).await;
    let p_a1 = permit(&db, eq_a1).await;
    let p_b = permit(&db, eq_b).await;
    let p_none = permit(&db, eq_a).await;
    // A row without a usable node (the column is NOT NULL: an old or received `0`).
    exec(
        &db,
        &format!("UPDATE work_permits SET entity_id = 0 WHERE id = {}", p_none.id),
    )
    .await;
    assert_eq!(
        (p_a.entity_id, p_a1.entity_id, p_b.entity_id),
        (t.site_a, t.line_a1, t.site_b)
    );
    Fixture {
        db,
        t,
        plant,
        site,
        closer_b,
        nobody,
        eq_a,
        eq_a1,
        eq_b,
        eq_none,
        wo_a,
        wo_b,
        wo_none,
        p_a,
        p_a1,
        p_b,
        p_none,
        before,
    }
}

/// The user's rights now (a fresh cache: roles change within a test).
async fn map_of(db: &DatabaseConnection, user_id: i32) -> AccessMap {
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    scope::access_map_cached(db, &cache, user_id).await.expect("access map")
}

fn all_filter() -> WorkPermitListFilter {
    WorkPermitListFilter {
        limit: Some(2000),
        ..WorkPermitListFilter::default()
    }
}

fn ids(page: &WorkPermitListPage) -> Vec<i64> {
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
// Lists and figures
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_site_user_lists_their_sites_permits_only_and_is_told_how_many_are_hidden() {
    let f = fixture().await;
    let site_scope = scope::access_scope(&f.db, f.site, PTW_VIEW).await.expect("scope");
    let plant_scope = scope::access_scope(&f.db, f.plant, PTW_VIEW).await.expect("scope");
    let (before_with_site, before_without_site) = f.before;

    let plant_page = queries::list_work_permits_in_scope(&f.db, all_filter(), &plant_scope)
        .await
        .expect("plant list");
    for p in [&f.p_a, &f.p_a1, &f.p_b, &f.p_none] {
        assert!(ids(&plant_page).contains(&p.id), "plant-wide: every permit");
    }
    assert_eq!(
        (plant_page.hidden_outside_scope, plant_page.hidden_without_site),
        (0, 0)
    );

    let site_page = queries::list_work_permits_in_scope(&f.db, all_filter(), &site_scope)
        .await
        .expect("site list");
    assert_eq!(
        ids(&site_page),
        sorted(vec![f.p_a.id, f.p_a1.id]),
        "site A and its line"
    );
    let expected_hidden = (before_with_site + 1, before_without_site + 1);
    assert_eq!(
        (site_page.hidden_outside_scope, site_page.hidden_without_site),
        expected_hidden,
        "site B's permit and the one without site, counted"
    );

    // Filtered in SQL before the limit: one row asked, one of site A's returned.
    let first = queries::list_work_permits_in_scope(
        &f.db,
        WorkPermitListFilter {
            limit: Some(1),
            ..WorkPermitListFilter::default()
        },
        &site_scope,
    )
    .await
    .expect("limit 1");
    assert_eq!(first.items.len(), 1);
    assert!([f.p_a.id, f.p_a1.id].contains(&first.items[0].id));

    // The hidden counts ignore the filters: searching site B's equipment (or anything) tells a
    // site-A user nothing.
    for search in ["PTW-SCOPE-B", "no permit says this"] {
        let searched = queries::list_work_permits_in_scope(
            &f.db,
            WorkPermitListFilter {
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
    let by_asset = queries::list_work_permits_in_scope(
        &f.db,
        WorkPermitListFilter {
            asset_id: Some(f.eq_b),
            ..all_filter()
        },
        &site_scope,
    )
    .await
    .expect("asset filter");
    assert!(by_asset.items.is_empty(), "the filter is intersected with the scope");
    assert_eq!(
        (by_asset.hidden_outside_scope, by_asset.hidden_without_site),
        expected_hidden
    );

    // A right held nowhere lists nothing.
    let nowhere = scope::access_scope(&f.db, f.nobody, PTW_VIEW).await.expect("scope");
    let empty = queries::list_work_permits_in_scope(&f.db, all_filter(), &nowhere)
        .await
        .expect("nowhere");
    assert!(empty.items.is_empty());

    // The unscoped entry point (internal callers) is plant-wide as before.
    let unscoped = queries::list_work_permits(&f.db, all_filter()).await.expect("unscoped");
    assert_eq!(ids(&unscoped), ids(&plant_page));
}

#[tokio::test]
async fn the_open_permits_report_and_the_compliance_figure_cover_the_users_sites_only() {
    let f = fixture().await;
    let site_scope = scope::access_scope(&f.db, f.site, PTW_VIEW).await.expect("scope");
    let plant_scope = scope::access_scope(&f.db, f.plant, PTW_VIEW).await.expect("scope");
    let plant_kpi_before = queries::permit_compliance_kpi_30d(&f.db).await.expect("kpi");
    let site_kpi_before = queries::permit_compliance_kpi_30d_in_scope(&f.db, &site_scope)
        .await
        .expect("kpi");
    assert_eq!(site_kpi_before.activated_count, 0);

    activate(&f.db, f.p_a.clone()).await;
    activate(&f.db, f.p_b.clone()).await;
    let p_none = queries::get_work_permit(&f.db, f.p_none.id)
        .await
        .expect("get")
        .expect("row");
    activate(&f.db, p_none).await;

    let site_open = queries::list_open_permits_report_in_scope(&f.db, &site_scope)
        .await
        .expect("site report");
    assert_eq!(ids(&site_open), vec![f.p_a.id], "site A's open permit only");
    assert!(site_open.hidden_outside_scope >= 1);
    assert!(site_open.hidden_without_site >= 1);
    let plant_open = queries::list_open_permits_report_in_scope(&f.db, &plant_scope)
        .await
        .expect("plant report");
    for id in [f.p_a.id, f.p_b.id, f.p_none.id] {
        assert!(ids(&plant_open).contains(&id));
    }
    assert_eq!(
        (plant_open.hidden_outside_scope, plant_open.hidden_without_site),
        (0, 0)
    );
    let unscoped = queries::list_open_permits_report(&f.db).await.expect("unscoped");
    assert_eq!(
        ids(&unscoped),
        ids(&plant_open),
        "home supervision: plant-wide as before"
    );

    let site_kpi = queries::permit_compliance_kpi_30d_in_scope(&f.db, &site_scope)
        .await
        .expect("site kpi");
    assert_eq!(
        site_kpi.activated_count, 1,
        "site A's activation only, never plant-wide"
    );
    let plant_kpi = queries::permit_compliance_kpi_30d_in_scope(&f.db, &plant_scope)
        .await
        .expect("plant kpi");
    assert_eq!(plant_kpi.activated_count, plant_kpi_before.activated_count + 3);
    let unscoped_kpi = queries::permit_compliance_kpi_30d(&f.db).await.expect("kpi");
    assert_eq!(unscoped_kpi.activated_count, plant_kpi.activated_count);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Open, create, edit, act
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn opening_a_permit_and_what_belongs_to_it_is_checked_at_its_site_with_the_refusals_in_order() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let nobody = map_of(&f.db, f.nobody).await;

    for p in [&f.p_a, &f.p_a1] {
        access::check_on_permit(&f.db, &site, PTW_VIEW, p.id)
            .await
            .expect("own site and below");
    }
    let err = access::check_on_permit(&f.db, &site, PTW_VIEW, f.p_b.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_permit(&f.db, &site, PTW_VIEW, f.p_none.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    for p in [&f.p_a, &f.p_a1, &f.p_b, &f.p_none] {
        access::check_on_permit(&f.db, &plant, PTW_VIEW, p.id)
            .await
            .expect("plant-wide: everywhere");
    }

    // Order: the right held nowhere first, then not found, then the site.
    let missing = 9_999_999;
    let err = access::check_on_permit(&f.db, &nobody, PTW_VIEW, missing)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    let err = access::check_on_permit(&f.db, &nobody, PTW_VIEW, f.p_b.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    let err = access::check_on_permit(&f.db, &site, PTW_VIEW, missing)
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");

    // Lists of what belongs to a permit: a permit gone has no site.
    access::check_on_permit_records(&f.db, &site, PTW_VIEW, f.p_a.id)
        .await
        .expect("own site");
    let err = access::check_on_permit_records(&f.db, &site, PTW_VIEW, f.p_b.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_permit_records(&f.db, &site, PTW_VIEW, missing)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    access::check_on_permit_records(&f.db, &plant, PTW_VIEW, missing)
        .await
        .expect("plant-wide reads what is left, as before");

    // Children addressed by their own id: at their permit's site.
    let iso_a = super::tests::add_isolation(&f.db, f.p_a.id, "Breaker A").await;
    let iso_b = super::tests::add_isolation(&f.db, f.p_b.id, "Breaker B").await;
    access::check_on_child(&f.db, &site, PTW_VIEW, PermitChild::Isolation, iso_a.id)
        .await
        .expect("own site");
    let err = access::check_on_child(&f.db, &site, PTW_VIEW, PermitChild::Isolation, iso_b.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_child(&f.db, &site, PTW_VIEW, PermitChild::Isolation, missing)
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");
    let err = access::check_on_child(&f.db, &nobody, PTW_VIEW, PermitChild::Isolation, iso_a.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    assert_eq!(
        access::child_anchor(&f.db, PermitChild::Isolation, iso_b.id)
            .await
            .expect("anchor"),
        Anchor::Node(f.t.site_b)
    );
}

#[tokio::test]
async fn creating_a_permit_needs_the_right_at_its_site_its_equipment_and_its_work_order() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let viewer = {
        let r = role(&f.db, "PTW scope viewer A", &[PTW_VIEW]).await;
        let u = user(&f.db, "ptw_viewer").await;
        assign(&f.db, u, r, "org_node", Some(f.t.site_a)).await;
        map_of(&f.db, u).await
    };
    let ty = hot_work(&f.db).await;

    // Allowed: equipment of site A or below, a node of site A, site A's work order.
    for input in [
        create_input(ty, Some(f.eq_a), None, None),
        create_input(ty, Some(f.eq_a1), None, None),
        create_input(ty, Some(f.eq_a1), Some(f.t.site_a), None),
        create_input(ty, None, None, Some(f.wo_a)),
    ] {
        access::check_create_at(&f.db, &site, &input)
            .await
            .unwrap_or_else(|e| panic!("{input:?}: {e:?}"));
    }
    // Refused: equipment, node or work order of site B; a work order or equipment without site.
    for (input, expected) in [
        (create_input(ty, Some(f.eq_b), None, None), "SCOPE_OUTSIDE_SITE"),
        (
            create_input(ty, Some(f.eq_b), Some(f.t.site_a), None),
            "SCOPE_OUTSIDE_SITE",
        ),
        (
            create_input(ty, Some(f.eq_a), Some(f.t.site_b), None),
            "SCOPE_OUTSIDE_SITE",
        ),
        (
            create_input(ty, Some(f.eq_a), Some(f.t.site_a), Some(f.wo_b)),
            "SCOPE_OUTSIDE_SITE",
        ),
        (
            create_input(ty, Some(f.eq_a), Some(f.t.site_a), Some(f.wo_none)),
            "SCOPE_RECORD_WITHOUT_SITE",
        ),
        (
            create_input(ty, Some(f.eq_none), None, None),
            "SCOPE_RECORD_WITHOUT_SITE",
        ),
    ] {
        let err = access::check_create_at(&f.db, &site, &input).await.unwrap_err();
        assert_eq!(code_of(&err), expected, "{input:?}");
        // Plant-wide: unchanged.
        access::check_create_at(&f.db, &plant, &input)
            .await
            .unwrap_or_else(|e| panic!("plant {input:?}: {e:?}"));
    }
    // The view right alone does not issue; a missing work order is not found.
    let err = access::check_create_at(&f.db, &viewer, &create_input(ty, Some(f.eq_a), None, None))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    let err = access::check_create_at(&f.db, &site, &create_input(ty, None, None, Some(9_999_999)))
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");
}

#[tokio::test]
async fn editing_a_permit_is_checked_at_its_site_and_at_every_new_place() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let edit = |id: i64, asset: Option<i64>, entity: Option<i64>, wo: Option<i64>| WorkPermitUpdateInput {
        id,
        linked_work_order_id: wo,
        asset_id: asset,
        entity_id: entity,
        expires_at: None,
        expected_row_version: 1,
    };

    for input in [
        edit(f.p_a.id, None, None, None),
        edit(f.p_a.id, Some(f.eq_a1), None, None),
        edit(f.p_a.id, None, Some(f.t.line_a1), None),
        edit(f.p_a.id, None, None, Some(f.wo_a)),
    ] {
        access::check_update(&f.db, &site, &input)
            .await
            .unwrap_or_else(|e| panic!("{input:?}: {e:?}"));
    }
    for (input, expected) in [
        (edit(f.p_b.id, None, None, None), "SCOPE_OUTSIDE_SITE"),
        (edit(f.p_none.id, None, None, None), "SCOPE_RECORD_WITHOUT_SITE"),
        (edit(f.p_a.id, Some(f.eq_b), None, None), "SCOPE_OUTSIDE_SITE"),
        (edit(f.p_a.id, Some(f.eq_none), None, None), "SCOPE_RECORD_WITHOUT_SITE"),
        (edit(f.p_a.id, None, Some(f.t.site_b), None), "SCOPE_OUTSIDE_SITE"),
        (edit(f.p_a.id, None, None, Some(f.wo_b)), "SCOPE_OUTSIDE_SITE"),
        (edit(f.p_a.id, None, None, Some(f.wo_none)), "SCOPE_RECORD_WITHOUT_SITE"),
    ] {
        let err = access::check_update(&f.db, &site, &input).await.unwrap_err();
        assert_eq!(code_of(&err), expected, "{input:?}");
        access::check_update(&f.db, &plant, &input)
            .await
            .unwrap_or_else(|e| panic!("plant {input:?}: {e:?}"));
    }
    let err = access::check_update(&f.db, &site, &edit(f.p_a.id, None, None, Some(9_999_999)))
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");
    let err = access::check_update(&f.db, &site, &edit(9_999_999, None, None, None))
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");
}

#[tokio::test]
async fn isolations_and_hand_overs_are_checked_at_their_permits_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let closer_b = map_of(&f.db, f.closer_b).await;
    let iso_a = super::tests::add_isolation(&f.db, f.p_a.id, "Breaker A").await;
    let iso_b = super::tests::add_isolation(&f.db, f.p_b.id, "Breaker B").await;

    // Define: a new isolation at its permit; an existing one at its own permit and the one named.
    access::check_isolation_upsert(&f.db, &site, None, f.p_a.id)
        .await
        .expect("new on site A");
    access::check_isolation_upsert(&f.db, &site, Some(iso_a.id), f.p_a.id)
        .await
        .expect("edit on site A");
    for (id, permit) in [(None, f.p_b.id), (Some(iso_b.id), f.p_a.id), (Some(iso_a.id), f.p_b.id)] {
        let err = access::check_isolation_upsert(&f.db, &site, id, permit)
            .await
            .unwrap_err();
        assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE", "{id:?} {permit}");
        access::check_isolation_upsert(&f.db, &plant, id, permit)
            .await
            .expect("plant-wide");
    }
    let err = access::check_isolation_upsert(&f.db, &site, None, f.p_none.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");

    // Apply, verify (ptw.issue) and lock-removal verification (ptw.close): at the permit's site.
    for permission in [PTW_ISSUE, PTW_CLOSE] {
        access::check_on_child(&f.db, &site, permission, PermitChild::Isolation, iso_a.id)
            .await
            .expect("site A");
        let err = access::check_on_child(&f.db, &site, permission, PermitChild::Isolation, iso_b.id)
            .await
            .unwrap_err();
        assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    }
    // A closer of site B removes site B's locks only, and issues nothing.
    access::check_on_child(&f.db, &closer_b, PTW_CLOSE, PermitChild::Isolation, iso_b.id)
        .await
        .expect("site B closer");
    let err = access::check_on_child(&f.db, &closer_b, PTW_CLOSE, PermitChild::Isolation, iso_a.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_child(&f.db, &closer_b, PTW_ISSUE, PermitChild::Isolation, iso_b.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");

    // Status, suspension, hand-over: at the permit's site.
    for (permission, permit) in [(PTW_ISSUE, &f.p_a), (PTW_CLOSE, &f.p_a1)] {
        access::check_on_permit(&f.db, &site, permission, permit.id)
            .await
            .expect("site A");
    }
    let err = access::check_on_permit(&f.db, &site, PTW_CLOSE, f.p_b.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let err = access::check_on_permit(&f.db, &site, PTW_CLOSE, f.p_none.id)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    access::check_on_permit(&f.db, &plant, PTW_CLOSE, f.p_none.id)
        .await
        .expect("plant-wide");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hand-over receivers (CMMS Advisor decision, option A)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn hand_over_receivers_hold_the_close_right_at_the_permits_site() {
    let f = fixture().await;
    let (plant, site, closer_b, nobody) = (
        i64::from(f.plant),
        i64::from(f.site),
        i64::from(f.closer_b),
        i64::from(f.nobody),
    );

    // Who may receive: site A's permits — the site A closer and plant-wide holders; site B's — the
    // site B closer and plant-wide holders; a permit without site — plant-wide holders only.
    for (permit, may, may_not) in [
        (&f.p_a1, vec![site, plant], vec![closer_b, nobody]),
        (&f.p_b, vec![closer_b, plant], vec![site, nobody]),
        (&f.p_none, vec![plant], vec![site, closer_b, nobody]),
    ] {
        for u in may {
            assert!(
                access::may_receive_handover(&f.db, permit.id, u).await.expect("check"),
                "{u} may receive {}",
                permit.id
            );
        }
        for u in may_not {
            assert!(
                !access::may_receive_handover(&f.db, permit.id, u).await.expect("check"),
                "{u} may not receive {}",
                permit.id
            );
        }
    }

    // The picker lists the same people, never the person asking.
    let page = queries::list_handover_receivers(&f.db, site, f.p_a.id, None, None, None)
        .await
        .expect("receivers");
    let listed: Vec<i64> = page.items.iter().map(|r| r.id).collect();
    assert!(listed.contains(&plant) && !listed.contains(&site));
    assert!(!listed.contains(&closer_b) && !listed.contains(&nobody));
    assert_eq!(page.total, i64::try_from(listed.len()).expect("len"));
    assert_eq!(page.empty_reason, None);
    let page = queries::list_handover_receivers(&f.db, closer_b, f.p_b.id, None, None, None)
        .await
        .expect("receivers");
    let listed: Vec<i64> = page.items.iter().map(|r| r.id).collect();
    assert!(listed.contains(&plant) && !listed.contains(&site) && !listed.contains(&closer_b));
    let err = queries::list_handover_receivers(&f.db, site, 9_999_999, None, None, None)
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");

    // Nobody else at the site: a coded empty state, not a silent empty list.
    exec(
        &f.db,
        &format!("UPDATE user_accounts SET is_active = 0 WHERE id <> {closer_b}"),
    )
    .await;
    let page = queries::list_handover_receivers(&f.db, closer_b, f.p_b.id, None, None, None)
        .await
        .expect("receivers");
    assert!(page.items.is_empty());
    assert_eq!(page.empty_reason.as_deref(), Some(queries::NO_RECEIVER_AT_SITE));
}

#[tokio::test]
async fn saving_a_hand_over_refuses_a_receiver_without_the_close_right_at_the_permits_site() {
    let f = fixture().await;
    let mut wp = activate(&f.db, f.p_a.clone()).await;
    wp = set_status(&f.db, &wp, "closed").await;
    super::tests::add_handover_role(&f.db, "Maintenance").await;
    super::tests::add_handover_role(&f.db, "Operations").await;
    let input = |receiver: i32| PermitHandoverLogInput {
        permit_id: wp.id,
        handed_from_role: "Maintenance".into(),
        handed_to_role: "Operations".into(),
        confirmation_note: "Zone left clean".into(),
        received_by_id: Some(i64::from(receiver)),
    };
    for receiver in [f.closer_b, f.nobody] {
        let err = queries::append_permit_handover_log(&f.db, input(receiver), i64::from(f.plant))
            .await
            .unwrap_err();
        assert_eq!(code_of(&err), "PERMIT_HANDOVER_RECEIVER_NO_RIGHT", "{receiver}");
    }
    // Receiver == signer is still refused first, whatever their rights.
    let err = queries::append_permit_handover_log(&f.db, input(f.site), i64::from(f.site))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMIT_HANDOVER_RECEIVER_IS_SIGNER");
    let log = queries::append_permit_handover_log(&f.db, input(f.site), i64::from(f.plant))
        .await
        .expect("site A closer receives site A's permit");
    assert_eq!(log.received_by_id, Some(i64::from(f.site)));
}

/// A permit's site follows its new equipment (or work order), derived as at creation: the right is
/// checked at the node that will be stored, and at the old one; the save stores that node, refuses
/// another one given with the change, and records the move with the session user.
#[tokio::test]
async fn changing_a_permits_equipment_moves_it_to_the_equipments_site_with_the_right_at_both() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    let plant = map_of(&f.db, f.plant).await;
    let edit = |wp: &WorkPermit, asset: Option<i64>, entity: Option<i64>, wo: Option<i64>| WorkPermitUpdateInput {
        id: wp.id,
        linked_work_order_id: wo,
        asset_id: asset,
        entity_id: entity,
        expires_at: None,
        expected_row_version: wp.row_version,
    };

    // Site A → site B: the site user is refused (the stored node is site B), plant-wide allowed.
    let to_b = edit(&f.p_a, Some(f.eq_b), None, None);
    let err = access::check_update(&f.db, &site, &to_b).await.unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    // Giving site A with equipment of site B does not help: the node stored is site B's anyway.
    let err = access::check_update(&f.db, &site, &edit(&f.p_a, Some(f.eq_b), Some(f.t.site_a), None))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    // Site B → site A: the site user holds the right at the new site but not at the old one.
    let err = access::check_update(&f.db, &site, &edit(&f.p_b, Some(f.eq_a), None, None))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    access::check_update(&f.db, &plant, &to_b).await.expect("plant-wide");

    let moved = queries::update_work_permit(&f.db, to_b, i64::from(f.plant))
        .await
        .expect("move with the equipment");
    assert_eq!((moved.asset_id, moved.entity_id), (f.eq_b, f.t.site_b));
    let audited = scalar(
        &f.db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'permit.site_moved' AND target_id = '{}' \
             AND actor_id = {} AND json_extract(details_json, '$.old_entity_id') = {} \
             AND json_extract(details_json, '$.new_entity_id') = {} \
             AND json_extract(details_json, '$.old_equipment_id') = {} \
             AND json_extract(details_json, '$.new_equipment_id') = {}",
            moved.id, f.plant, f.t.site_a, f.t.site_b, f.eq_a, f.eq_b
        ),
    )
    .await;
    assert_eq!(audited, 1);

    // Within site A: to equipment of line A1, the permit moves to the line (allowed to the site user).
    access::check_update(&f.db, &site, &edit(&f.p_a1, Some(f.eq_a), None, None))
        .await
        .expect("line A1 to site A, both in the user's site");
    let back = queries::update_work_permit(&f.db, edit(&f.p_a1, Some(f.eq_a), None, None), i64::from(f.site))
        .await
        .expect("line to site");
    assert_eq!(back.entity_id, f.t.site_a);

    // Another site given with the change is refused, even plant-wide.
    let p = permit(&f.db, f.eq_a).await;
    let err = queries::update_work_permit(
        &f.db,
        edit(&p, Some(f.eq_a1), Some(f.t.site_b), None),
        i64::from(f.plant),
    )
    .await
    .unwrap_err();
    assert_eq!(code_of(&err), "PERMIT_SITE_NOT_EQUIPMENT_SITE");
    // Its own new site may be given.
    let ok = queries::update_work_permit(
        &f.db,
        edit(&p, Some(f.eq_a1), Some(f.t.line_a1), None),
        i64::from(f.plant),
    )
    .await
    .expect("the equipment's own site");
    assert_eq!(ok.entity_id, f.t.line_a1);

    // Linked to a work order of site B (on equipment of site B): the permit takes the work order's site.
    let q = permit(&f.db, f.eq_a).await;
    let err = access::check_update(&f.db, &site, &edit(&q, Some(f.eq_b), None, Some(f.wo_b)))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE");
    let on_wo = queries::update_work_permit(&f.db, edit(&q, Some(f.eq_b), None, Some(f.wo_b)), i64::from(f.plant))
        .await
        .expect("onto the work order");
    assert_eq!(
        (on_wo.asset_id, on_wo.entity_id, on_wo.linked_work_order_id),
        (f.eq_b, f.t.site_b, Some(f.wo_b))
    );
    // Equipment installed nowhere: plant-wide users only at the check, and no site to store.
    let r = permit(&f.db, f.eq_a).await;
    let err = access::check_update(&f.db, &site, &edit(&r, Some(f.eq_none), None, None))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_RECORD_WITHOUT_SITE");
    assert!(
        queries::update_work_permit(&f.db, edit(&r, Some(f.eq_none), None, None), i64::from(f.plant))
            .await
            .is_err()
    );
}

/// Read-only review of permits approved before the site followed the equipment: an approved,
/// issued or active permit whose equipment is on another site is listed by the data-integrity
/// check, never rewritten; a permit on a site for equipment of one of its lines is not listed.
#[tokio::test]
async fn permits_on_equipment_of_another_site_are_listed_for_review_not_rewritten() {
    let f = fixture().await;
    let on_b = set_status(
        &f.db,
        &set_status(&f.db, &permit(&f.db, f.eq_a).await, "pending_review").await,
        "approved",
    )
    .await;
    // An old row: approved on site A while its equipment is installed on site B.
    exec(
        &f.db,
        &format!("UPDATE work_permits SET asset_id = {} WHERE id = {}", f.eq_b, on_b.id),
    )
    .await;
    // On site A, for equipment of line A1: the same site.
    let line = set_status(&f.db, &permit(&f.db, f.eq_a1).await, "pending_review").await;
    let line = set_status(&f.db, &line, "approved").await;
    exec(
        &f.db,
        &format!(
            "UPDATE work_permits SET entity_id = {} WHERE id = {}",
            f.t.site_a, line.id
        ),
    )
    .await;
    // A draft in the same case is not judged (its equipment can still change).
    let draft = permit(&f.db, f.eq_a).await;
    exec(
        &f.db,
        &format!("UPDATE work_permits SET asset_id = {} WHERE id = {}", f.eq_b, draft.id),
    )
    .await;

    crate::data_integrity::run_data_integrity_detectors(&f.db)
        .await
        .expect("detectors");
    let listed = |id: i64| {
        format!(
            "SELECT COUNT(*) FROM data_integrity_findings WHERE finding_code = 'PERMIT_SITE_NOT_EQUIPMENT_SITE' \
             AND record_class = 'work_permits' AND record_id = {id} AND status = 'open'"
        )
    };
    assert_eq!(scalar(&f.db, &listed(on_b.id)).await, 1);
    assert_eq!(scalar(&f.db, &listed(line.id)).await, 0);
    assert_eq!(scalar(&f.db, &listed(draft.id)).await, 0);
    // Never rewritten.
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT entity_id FROM work_permits WHERE id = {}", on_b.id)
        )
        .await,
        f.t.site_a
    );
    // Running again does not duplicate it.
    crate::data_integrity::run_data_integrity_detectors(&f.db)
        .await
        .expect("detectors again");
    assert_eq!(scalar(&f.db, &listed(on_b.id)).await, 1);
}
