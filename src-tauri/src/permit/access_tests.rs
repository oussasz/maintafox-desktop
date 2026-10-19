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
    // The report's hidden counts cover only what it would show (open, not expired): the drafts of
    // site B and without site are not counted yet.
    let site_open_before = queries::list_open_permits_report_in_scope(&f.db, &site_scope)
        .await
        .expect("site report");
    let hidden_before = (
        site_open_before.hidden_outside_scope,
        site_open_before.hidden_without_site,
    );

    activate(&f.db, f.p_a.clone()).await;
    let p_b = activate(&f.db, f.p_b.clone()).await;
    let p_none = queries::get_work_permit(&f.db, f.p_none.id)
        .await
        .expect("get")
        .expect("row");
    activate(&f.db, p_none).await;

    let site_open = queries::list_open_permits_report_in_scope(&f.db, &site_scope)
        .await
        .expect("site report");
    assert_eq!(ids(&site_open), vec![f.p_a.id], "site A's open permit only");
    assert_eq!(
        (site_open.hidden_outside_scope, site_open.hidden_without_site),
        (hidden_before.0 + 1, hidden_before.1 + 1),
        "site B's and the siteless permit, now open, are counted"
    );
    // An expired open permit is not shown, so not counted either.
    exec(
        &f.db,
        &format!(
            "UPDATE work_permits SET expires_at = '2000-01-01T00:00:00Z' WHERE id = {}",
            p_b.id
        ),
    )
    .await;
    let after_expiry = queries::list_open_permits_report_in_scope(&f.db, &site_scope)
        .await
        .expect("site report");
    assert_eq!(
        (after_expiry.hidden_outside_scope, after_expiry.hidden_without_site),
        (hidden_before.0, hidden_before.1 + 1),
        "site B's expired permit is not counted"
    );
    exec(
        &f.db,
        &format!(
            "UPDATE work_permits SET expires_at = '{}' WHERE id = {}",
            super::tests::future_expiry(),
            p_b.id
        ),
    )
    .await;
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
    // Plant-wide: the scope passes, then the create's own site rule decides (the same function as
    // the create: `None` allowed, else the refusal code, `VALIDATION` for a site not derivable).
    for (input, expected, plant_wide) in [
        (create_input(ty, Some(f.eq_b), None, None), "SCOPE_OUTSIDE_SITE", None),
        (
            create_input(ty, Some(f.eq_b), Some(f.t.site_a), None),
            "SCOPE_OUTSIDE_SITE",
            Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"),
        ),
        (
            create_input(ty, Some(f.eq_a), Some(f.t.site_b), None),
            "SCOPE_OUTSIDE_SITE",
            Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"),
        ),
        // The work order's site (B) is the derived one: site A is off its chain.
        (
            create_input(ty, Some(f.eq_a), Some(f.t.site_a), Some(f.wo_b)),
            "SCOPE_OUTSIDE_SITE",
            Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"),
        ),
        // A work order recording no node: the equipment's site (A) is the derived one.
        (
            create_input(ty, Some(f.eq_a), Some(f.t.site_a), Some(f.wo_none)),
            "SCOPE_RECORD_WITHOUT_SITE",
            None,
        ),
        (
            create_input(ty, Some(f.eq_none), None, None),
            "SCOPE_RECORD_WITHOUT_SITE",
            Some("VALIDATION"),
        ),
        // Equipment installed nowhere on a node of the user's site: refused as an edit refuses it.
        (
            create_input(ty, Some(f.eq_none), Some(f.t.site_a), None),
            "SCOPE_RECORD_WITHOUT_SITE",
            Some("VALIDATION"),
        ),
    ] {
        let err = access::check_create_at(&f.db, &site, &input).await.unwrap_err();
        assert_eq!(code_of(&err), expected, "{input:?}");
        let plant_result = access::check_create_at(&f.db, &plant, &input).await;
        let create_result = queries::create_work_permit(&f.db, input.clone()).await;
        match plant_wide {
            None => {
                plant_result.unwrap_or_else(|e| panic!("plant {input:?}: {e:?}"));
                create_result.unwrap_or_else(|e| panic!("create {input:?}: {e:?}"));
            }
            Some("VALIDATION") => {
                let err = plant_result.unwrap_err();
                assert!(matches!(err, AppError::ValidationFailed(_)), "{input:?}: {err:?}");
                // The check and the create agree.
                assert_eq!(format!("{err:?}"), format!("{:?}", create_result.unwrap_err()));
            }
            Some(code) => {
                assert_eq!(code_of(&plant_result.unwrap_err()), code, "plant {input:?}");
                assert_eq!(code_of(&create_result.unwrap_err()), code, "create {input:?}");
            }
        }
    }
    // The view right alone does not issue.
    let err = access::check_create_at(&f.db, &viewer, &create_input(ty, Some(f.eq_a), None, None))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    // What the create reports first comes before the sites, for everyone: a work order or
    // equipment that does not exist is not found (even with another site's node given), no
    // equipment given nor on a work order is the create's validation error.
    let missing = 9_999_999;
    for map in [&site, &plant] {
        for input in [
            create_input(ty, None, None, Some(missing)),
            create_input(ty, None, Some(f.t.site_b), Some(missing)),
            create_input(ty, Some(missing), None, None),
            create_input(ty, Some(missing), Some(f.t.site_a), None),
            create_input(ty, Some(missing), Some(f.t.site_b), None),
        ] {
            let err = access::check_create_at(&f.db, map, &input).await.unwrap_err();
            assert!(is_not_found(&err), "{input:?}: {err:?}");
        }
        for input in [
            create_input(ty, None, None, None),
            create_input(ty, None, Some(f.t.site_b), None),
        ] {
            let err = access::check_create_at(&f.db, map, &input).await.unwrap_err();
            assert!(matches!(err, AppError::ValidationFailed(_)), "{input:?}: {err:?}");
            // The same error as the create itself.
            let create_err = queries::create_work_permit(&f.db, input.clone()).await.unwrap_err();
            assert_eq!(format!("{err:?}"), format!("{create_err:?}"));
        }
    }
    let create_err = queries::create_work_permit(&f.db, create_input(ty, Some(missing), None, None))
        .await
        .unwrap_err();
    assert!(is_not_found(&create_err), "{create_err:?}");
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

/// Whether `u` may receive `permit`, asserted to agree with the user's real access map (the
/// close right at the permit's node, as every other check reads it).
async fn eligible_as_the_access_map_says(db: &DatabaseConnection, permit: &WorkPermit, u: i32) -> bool {
    let may = access::may_receive_handover(db, permit.id, i64::from(u))
        .await
        .expect("check");
    let close = scope::access_scope(db, u, PTW_CLOSE).await.expect("scope");
    let held = scope::check_anchor(db, &close, PTW_CLOSE, Anchor::of(Some(permit.entity_id)))
        .await
        .is_ok();
    assert_eq!(
        may, held,
        "user {u}, permit {}: receiver rule and access map disagree",
        permit.id
    );
    may
}

/// A new user with `role_id` on `node`, the assignment then changed by `change` (an SQL `SET` list).
async fn assigned_on(db: &DatabaseConnection, name: &str, role_id: i64, node: i64, change: Option<&str>) -> i32 {
    let u = user(db, name).await;
    assign(db, u, role_id, "org_node", Some(node)).await;
    if let Some(set) = change {
        exec(
            db,
            &format!("UPDATE user_scope_assignments SET {set} WHERE user_id = {u}"),
        )
        .await;
    }
    u
}

/// The receiver rule (picker and save) reads assignments exactly as the access map does: the close
/// right itself, the subtree going down only, assignments in force (validity, deletion, emergency
/// expiry), live nodes, strict node references, and elevated administrators.
#[tokio::test]
async fn hand_over_receivers_follow_the_same_assignment_rules_as_the_access_map() {
    let f = fixture().await;
    let close_role = role(&f.db, "PTW scope closer only", &[PTW_CLOSE]).await;
    let view_role = role(&f.db, "PTW scope viewer only", &[PTW_VIEW]).await;
    let site_a = f.t.site_a;

    // Refused for site A's permit:
    let view_only = assigned_on(&f.db, "ptw_rcv_view_only", view_role, site_a, None).await;
    let line_only = assigned_on(&f.db, "ptw_rcv_line_only", close_role, f.t.line_a1, None).await;
    let ended = assigned_on(
        &f.db,
        "ptw_rcv_ended",
        close_role,
        site_a,
        Some("valid_to = '2000-01-01T00:00:00Z'"),
    )
    .await;
    let not_started = assigned_on(
        &f.db,
        "ptw_rcv_not_started",
        close_role,
        site_a,
        Some("valid_from = '2999-01-01T00:00:00Z'"),
    )
    .await;
    let deleted = assigned_on(
        &f.db,
        "ptw_rcv_deleted",
        close_role,
        site_a,
        Some("deleted_at = '2026-01-02T00:00:00Z'"),
    )
    .await;
    let lapsed = assigned_on(
        &f.db,
        "ptw_rcv_lapsed_emergency",
        close_role,
        site_a,
        Some("is_emergency = 1, emergency_expires_at = '2000-01-01T00:00:00Z'"),
    )
    .await;
    let emergency_no_expiry = assigned_on(
        &f.db,
        "ptw_rcv_emergency_no_expiry",
        close_role,
        site_a,
        Some("is_emergency = 1, emergency_expires_at = NULL"),
    )
    .await;
    // A malformed node reference that a lenient cast would read as site A.
    let malformed_ref = format!("scope_reference = '{site_a}x'");
    let malformed = assigned_on(&f.db, "ptw_rcv_malformed", close_role, site_a, Some(&malformed_ref)).await;

    // Eligible for site A's permit:
    let live_emergency = assigned_on(
        &f.db,
        "ptw_rcv_live_emergency",
        close_role,
        site_a,
        Some("is_emergency = 1, emergency_expires_at = '2999-01-01T00:00:00Z'"),
    )
    .await;
    // Padding around a well-formed reference is read as the node (Rust `trim`, then parse).
    let padded_ref = format!("scope_reference = ' {site_a} '");
    let padded = assigned_on(&f.db, "ptw_rcv_padded", close_role, site_a, Some(&padded_ref)).await;
    let on_plant_node = user(&f.db, "ptw_rcv_plant_node").await;
    assign(&f.db, on_plant_node, close_role, "org_node", Some(f.t.plant)).await;
    // An elevated administrator holds every right, with no role at all.
    let admin = user(&f.db, "ptw_rcv_admin").await;
    exec(
        &f.db,
        &format!("UPDATE user_accounts SET is_admin = 1 WHERE id = {admin}"),
    )
    .await;

    let refused = [
        view_only,
        line_only,
        ended,
        not_started,
        deleted,
        lapsed,
        emergency_no_expiry,
        malformed,
    ];
    let eligible = [f.site, f.plant, live_emergency, padded, on_plant_node, admin];
    for u in refused {
        assert!(
            !eligible_as_the_access_map_says(&f.db, &f.p_a, u).await,
            "{u} may not receive site A's permit"
        );
    }
    for u in eligible {
        assert!(
            eligible_as_the_access_map_says(&f.db, &f.p_a, u).await,
            "{u} may receive site A's permit"
        );
    }
    // The line closer receives the line's permit (the subtree goes down).
    assert!(eligible_as_the_access_map_says(&f.db, &f.p_a1, line_only).await);

    // The picker lists exactly the eligible people (the asker aside).
    let page = queries::list_handover_receivers(&f.db, i64::from(f.closer_b), f.p_a.id, None, None, None)
        .await
        .expect("receivers");
    let listed: Vec<i64> = page.items.iter().map(|r| r.id).collect();
    for u in eligible {
        assert!(listed.contains(&i64::from(u)), "{u} listed");
    }
    for u in refused {
        assert!(!listed.contains(&i64::from(u)), "{u} not listed");
    }

    // A role on an org node deleted since grants nothing, here as in the access map.
    let area = crate::auth::scope_tests::node(&f.db, "RCV-AREA", Some(f.t.plant)).await;
    let zone = crate::auth::scope_tests::node(&f.db, "RCV-ZONE", Some(area)).await;
    let eq_zone = equipment(&f.db, "RCV-ZONE", Some(zone)).await;
    let p_zone = permit(&f.db, eq_zone).await;
    let area_closer = user(&f.db, "ptw_rcv_area").await;
    assign(&f.db, area_closer, close_role, "org_node", Some(area)).await;
    assert!(eligible_as_the_access_map_says(&f.db, &p_zone, area_closer).await);
    exec(
        &f.db,
        &format!("UPDATE org_nodes SET deleted_at = '2026-01-02T00:00:00Z' WHERE id = {area}"),
    )
    .await;
    assert!(!eligible_as_the_access_map_says(&f.db, &p_zone, area_closer).await);

    // A reference beyond `i64` is no node (Rust's parse refuses it; a bare SQL cast would read it as
    // the largest id). Last: a node with the largest id ends the table's id sequence.
    let top = crate::auth::scope_tests::node(&f.db, "RCV-TOP", Some(f.t.plant)).await;
    exec(
        &f.db,
        &format!("UPDATE org_nodes SET id = {} WHERE id = {top}", i64::MAX),
    )
    .await;
    let p_top = permit(&f.db, equipment(&f.db, "RCV-TOP", Some(i64::MAX)).await).await;
    assert_eq!(p_top.entity_id, i64::MAX);
    let at_max = assigned_on(&f.db, "ptw_rcv_at_max", close_role, i64::MAX, None).await;
    assert!(eligible_as_the_access_map_says(&f.db, &p_top, at_max).await);
    for (name, reference) in [
        ("ptw_rcv_overflow", "9223372036854775808"),
        ("ptw_rcv_overflow_long", "99999999999999999999"),
    ] {
        let set = format!("scope_reference = '{reference}'");
        let overflow = assigned_on(&f.db, name, close_role, i64::MAX, Some(&set)).await;
        assert!(
            !eligible_as_the_access_map_says(&f.db, &p_top, overflow).await,
            "{reference} is no node"
        );
    }
}

/// LOTO card print: the isolation must belong to the permit named before the LOTO role is judged
/// on that permit (a mismatched permit is not found, never that permit's qualification refusal).
#[tokio::test]
async fn printing_a_loto_card_checks_the_isolation_belongs_to_the_permit_first() {
    let f = fixture().await;
    exec(
        &f.db,
        "INSERT INTO certification_types (entity_sync_id, code, name, category, row_version) \
         VALUES ('ct-ptw-scope-loto', 'PTW-SCOPE-LOTO', 'Chargé de consignation', 'loto', 1)",
    )
    .await;
    let loto = scalar(
        &f.db,
        "SELECT id FROM certification_types WHERE code = 'PTW-SCOPE-LOTO'",
    )
    .await;
    let admin = scalar(&f.db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    crate::qualification::requirements::upsert_requirement(
        &f.db,
        crate::qualification::requirements::QualificationRequirementUpsertInput {
            id: None,
            expected_row_version: None,
            target_kind: "loto_role".into(),
            work_order_id: None,
            target_code: Some("isolation_apply".into()),
            certification_type_id: Some(loto),
            skill_reference_value_id: None,
            min_proficiency_level: None,
            coverage: None,
            notes: None,
            is_active: None,
        },
        admin,
    )
    .await
    .expect("role requirement");
    let iso_a = super::tests::add_isolation(&f.db, f.p_a.id, "Breaker A").await;
    let print = |permit_id: i64| crate::permit::domain::LotoCardPrintInput {
        permit_id,
        isolation_id: iso_a.id,
    };
    // The plant user has no personnel record: the LOTO role gate refuses them on a matching pair.
    let err = queries::record_loto_card_print(&f.db, print(f.p_a.id), i64::from(f.plant))
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "LOTO_ROLE_PERSON_UNKNOWN");
    // Another permit named: not found, before any gate on that permit.
    let err = queries::record_loto_card_print(&f.db, print(f.p_b.id), i64::from(f.plant))
        .await
        .unwrap_err();
    assert!(is_not_found(&err), "{err:?}");
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
    // Giving site A with equipment of site B does not help: the right is needed at the new
    // equipment's site too (and the save refuses site A, off site B's chain).
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
    // A level above the new equipment's site may be given too.
    let above = queries::update_work_permit(
        &f.db,
        edit(&ok, Some(f.eq_a), Some(f.t.plant), None),
        i64::from(f.plant),
    )
    .await
    .expect("a level above the equipment's site");
    assert_eq!((above.asset_id, above.entity_id), (f.eq_a, f.t.plant));

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

/// The edit of a permit's site alone (same equipment and work order): before approval, the node
/// given must be its equipment's (or work order's) or a level above it — never a sibling, a level
/// below or another site; from approved on, the site is fixed. Audited like an equipment change.
#[tokio::test]
async fn a_permits_site_follows_its_equipments_chain_and_is_fixed_once_approved() {
    let f = fixture().await;
    let line_a2 = crate::auth::scope_tests::node(&f.db, "PTW-A2", Some(f.t.site_a)).await;
    let site_edit = |wp: &WorkPermit, entity: i64| WorkPermitUpdateInput {
        id: wp.id,
        linked_work_order_id: None,
        asset_id: None,
        entity_id: Some(entity),
        expires_at: None,
        expected_row_version: wp.row_version,
    };
    let actor = i64::from(f.plant);

    // Draft on line A1: up to site A and the plant, allowed and audited.
    let draft = permit(&f.db, f.eq_a1).await;
    let up = queries::update_work_permit(&f.db, site_edit(&draft, f.t.site_a), actor)
        .await
        .expect("a level above");
    assert_eq!(up.entity_id, f.t.site_a);
    let audited = scalar(
        &f.db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'permit.site_moved' AND target_id = '{}' \
             AND json_extract(details_json, '$.old_entity_id') = {} \
             AND json_extract(details_json, '$.new_entity_id') = {}",
            up.id, f.t.line_a1, f.t.site_a
        ),
    )
    .await;
    assert_eq!(audited, 1);
    let up = queries::update_work_permit(&f.db, site_edit(&up, f.t.plant), actor)
        .await
        .expect("the plant");
    // Back down to its equipment's own line: allowed (the derived node).
    let back = queries::update_work_permit(&f.db, site_edit(&up, f.t.line_a1), actor)
        .await
        .expect("its equipment's node");
    assert_eq!(back.entity_id, f.t.line_a1);
    // A sibling line, another site: refused, nothing written.
    for node in [line_a2, f.t.site_b] {
        let err = queries::update_work_permit(&f.db, site_edit(&back, node), actor)
            .await
            .unwrap_err();
        assert_eq!(code_of(&err), "PERMIT_SITE_NOT_EQUIPMENT_SITE", "node {node}");
    }
    // A level below the equipment's site: refused.
    let on_site = permit(&f.db, f.eq_a).await;
    let err = queries::update_work_permit(&f.db, site_edit(&on_site, f.t.line_a1), actor)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMIT_SITE_NOT_EQUIPMENT_SITE");
    let kept = queries::get_work_permit(&f.db, on_site.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(kept.entity_id, f.t.site_a);

    // Waiting for review: a level above is allowed and sends the permit back to draft (the review
    // judged it at its former site, and who issues it changes), audited as a review reset.
    let review = set_status(&f.db, &permit(&f.db, f.eq_a1).await, "pending_review").await;
    let moved = queries::update_work_permit(&f.db, site_edit(&review, f.t.site_a), actor)
        .await
        .expect("waiting for review");
    assert_eq!((moved.entity_id, moved.status.as_str()), (f.t.site_a, "draft"));
    let reset = scalar(
        &f.db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'permit.review_reset' AND target_id = '{}' \
             AND json_extract(details_json, '$.reason') = 'site_changed'",
            moved.id
        ),
    )
    .await;
    assert_eq!(reset, 1);
    // The former review no longer counts: approving now is refused, it is submitted again.
    let err = queries::set_work_permit_status(
        &f.db,
        WorkPermitStatusInput {
            id: moved.id,
            status: "approved".into(),
            expected_row_version: moved.row_version,
        },
        1,
    )
    .await
    .unwrap_err();
    assert_eq!(code_of(&err), "PERMIT_TRANSITION_NOT_ALLOWED");
    let moved = set_status(&f.db, &moved, "pending_review").await;

    // Approved, issued: the site is fixed, even to a level above; the same site given passes.
    let mut wp = set_status(&f.db, &moved, "approved").await;
    for status in ["approved", "issued"] {
        if status == "issued" {
            wp = set_status(&f.db, &wp, "issued").await;
        }
        for node in [f.t.plant, f.t.line_a1, f.t.site_b] {
            let err = queries::update_work_permit(&f.db, site_edit(&wp, node), actor)
                .await
                .unwrap_err();
            assert_eq!(code_of(&err), "PERMIT_SITE_LOCKED", "{status} to {node}");
        }
        wp = queries::update_work_permit(&f.db, site_edit(&wp, f.t.site_a), actor)
            .await
            .unwrap_or_else(|e| panic!("same site, {status}: {e:?}"));
        assert_eq!(wp.entity_id, f.t.site_a);
    }
}

/// The create refuses a node given off its equipment's (or work order's) chain, as the edit does.
#[tokio::test]
async fn creating_a_permit_on_a_node_off_its_equipments_chain_is_refused() {
    let f = fixture().await;
    let line_a2 = crate::auth::scope_tests::node(&f.db, "PTW-A2C", Some(f.t.site_a)).await;
    let plant = map_of(&f.db, f.plant).await;
    let ty = hot_work(&f.db).await;
    for (asset, node) in [(f.eq_a1, f.t.line_a1), (f.eq_a1, f.t.site_a), (f.eq_a1, f.t.plant)] {
        let input = create_input(ty, Some(asset), Some(node), None);
        access::check_create_at(&f.db, &plant, &input)
            .await
            .unwrap_or_else(|e| panic!("check {input:?}: {e:?}"));
        let wp = queries::create_work_permit(&f.db, input).await.expect("on the chain");
        assert_eq!(wp.entity_id, node);
    }
    for (asset, node) in [
        (f.eq_a1, line_a2),
        (f.eq_a1, f.t.site_b),
        (f.eq_a, f.t.line_a1),
        (f.eq_a, 0),
    ] {
        let input = create_input(ty, Some(asset), Some(node), None);
        let err = access::check_create_at(&f.db, &plant, &input).await.unwrap_err();
        assert_eq!(code_of(&err), "PERMIT_SITE_NOT_EQUIPMENT_SITE", "check {input:?}");
        let err = queries::create_work_permit(&f.db, input.clone()).await.unwrap_err();
        assert_eq!(code_of(&err), "PERMIT_SITE_NOT_EQUIPMENT_SITE", "create {input:?}");
    }
}

/// A work order recording no usable node (`0`): its permit's site cannot be derived, so the create
/// and the edit refuse it (no fall-back to the equipment's site, as on arrival by sync). A work
/// order recording no node at all (`NULL`) leaves the equipment's site as the derived one.
#[tokio::test]
async fn a_work_order_without_a_usable_site_gives_its_permit_no_site() {
    let f = fixture().await;
    let ty = hot_work(&f.db).await;
    let wo_zero = work_order(&f.db, f.eq_a, f.plant, "Work order on node 0").await;
    exec(
        &f.db,
        &format!("UPDATE work_orders SET entity_id = 0 WHERE id = {wo_zero}"),
    )
    .await;
    let err = queries::create_work_permit(&f.db, create_input(ty, None, None, Some(wo_zero)))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ValidationFailed(_)), "{err:?}");
    let draft = permit(&f.db, f.eq_a).await;
    let err = queries::update_work_permit(
        &f.db,
        WorkPermitUpdateInput {
            id: draft.id,
            linked_work_order_id: Some(wo_zero),
            asset_id: None,
            entity_id: None,
            expires_at: None,
            expected_row_version: draft.row_version,
        },
        i64::from(f.plant),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AppError::ValidationFailed(_)), "{err:?}");
    // NULL: the equipment's site.
    let on_null = queries::create_work_permit(&f.db, create_input(ty, None, None, Some(f.wo_none)))
        .await
        .expect("work order without a node");
    assert_eq!(on_null.entity_id, f.t.site_a);
}

/// The save stores only the target the scope check judged: when the equipment's site changes
/// between the check and the save, the edit is refused as a concurrent update, nothing written.
#[tokio::test]
async fn the_save_refuses_a_node_other_than_the_one_checked() {
    let f = fixture().await;
    let plant = map_of(&f.db, f.plant).await;
    let wp = permit(&f.db, f.eq_a).await;
    let input = WorkPermitUpdateInput {
        id: wp.id,
        linked_work_order_id: None,
        asset_id: Some(f.eq_a1),
        entity_id: None,
        expires_at: None,
        expected_row_version: wp.row_version,
    };
    let checked = access::check_update(&f.db, &plant, &input).await.expect("check");
    assert_eq!(checked.entity_id, Some(f.t.line_a1));
    // The equipment is moved to site B after the check.
    exec(
        &f.db,
        &format!(
            "UPDATE equipment SET installed_at_node_id = {} WHERE id = {}",
            f.t.site_b, f.eq_a1
        ),
    )
    .await;
    let err = queries::update_work_permit_checked(&f.db, input.clone(), i64::from(f.plant), &checked)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::ValidationFailed(m) if m.iter().any(|t| t.contains("Concurrent update"))),
        "{err:?}"
    );
    let kept = queries::get_work_permit(&f.db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!((kept.asset_id, kept.entity_id), (f.eq_a, f.t.site_a));
    // Checked again: the target now names site B, and is saved as checked.
    let checked = access::check_update(&f.db, &plant, &input).await.expect("check again");
    assert_eq!(checked.entity_id, Some(f.t.site_b));
    let saved = queries::update_work_permit_checked(&f.db, input, i64::from(f.plant), &checked)
        .await
        .expect("saved as checked");
    assert_eq!((saved.asset_id, saved.entity_id), (f.eq_a1, f.t.site_b));
}

/// Unknown new equipment is not found before any site refusal (as the create reports it), even
/// with a node given.
#[tokio::test]
async fn editing_onto_unknown_equipment_is_not_found_before_the_site() {
    let f = fixture().await;
    let site = map_of(&f.db, f.site).await;
    for entity in [None, Some(f.t.site_b)] {
        let input = WorkPermitUpdateInput {
            id: f.p_a.id,
            linked_work_order_id: None,
            asset_id: Some(9_999_999),
            entity_id: entity,
            expires_at: None,
            expected_row_version: f.p_a.row_version,
        };
        let err = access::check_update(&f.db, &site, &input).await.unwrap_err();
        assert!(is_not_found(&err), "{entity:?}: {err:?}");
    }
}
