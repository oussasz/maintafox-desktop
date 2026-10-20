//! "Assign a site" to old records without one (CMMS Advisor decision Q1, step 6): on the org tree of
//! `auth::scope_tests` (plant → site A → line A1, plant → site B), a plant-wide organisation
//! administrator gives a site to work orders, requests and planning windows that have none, whatever
//! their status, with one reason and one audit entry per record; a record that has a site, a user
//! without the right plant-wide, a deleted node and a missing reason are refused. On arrival by sync,
//! the assignment is accepted only when its author holds the right plant-wide here and the record
//! still has no site here, else a conflict.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Map, Value};

use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::org::site_assignment::{self, inbound_site_assignment_check, AssignSiteInput, NoSiteKind};
use crate::rbac::permissions::{ORG_ADMIN, ORG_VIEW};
use crate::settings::site::InboundVerdict;

const REASON: &str = "Old order of the pump house, recorded on site A";

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// `org.admin` plant-wide.
    admin: i32,
    /// `org.admin` on site A only.
    site_admin: i32,
    /// Organisation view plant-wide, no administration right.
    viewer: i32,
    /// Equipment installed on site A and on site B.
    eq_a: i64,
    eq_b: i64,
    /// Work orders without a site (one open, one closed) and one on site A.
    wo_none: i64,
    wo_closed_none: i64,
    wo_a: i64,
    /// A request stored with org node 0 (before decision O7).
    di_none: i64,
    /// Planning windows without a site: one flagged "Site to re-enter", one plant-wide on the
    /// equipment of site A.
    win_review: i64,
    win_eq: i64,
}

async fn equipment(db: &DatabaseConnection, code: &str, node: i64) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('sa-eq-{code}', 'SA-{code}', 'Equipment {code}', 'active_in_service', {node},
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'sa-eq-{code}'"),
    )
    .await
}

async fn work_order(db: &DatabaseConnection, equipment_id: i64, creator: i32, title: &str) -> i64 {
    crate::wo::queries::create_work_order(
        db,
        crate::wo::domain::WoCreateInput {
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
            description: Some(format!("{title} (site assignment test)")),
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

async fn request(db: &DatabaseConnection, equipment_id: i64, node: i64, submitter: i32, title: &str) -> i64 {
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("lookup")
        .expect("seeded symptom");
    crate::di::queries::create_intervention_request(
        db,
        crate::di::queries::DiCreateInput {
            equipment_id,
            org_node_id: node,
            title: title.into(),
            description: format!("{title} (site assignment test)"),
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
            submitter_id: i64::from(submitter),
            source_inspection_anomaly_id: None,
        },
    )
    .await
    .expect("create DI")
    .id
}

async fn window(db: &DatabaseConnection, key: &str, equipment: Option<i64>, review: bool) -> i64 {
    let equipment_sql = equipment.map_or_else(|| "NULL".to_string(), |e| e.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO planning_windows (entity_sync_id, entity_id, equipment_id, window_type, start_datetime,
                end_datetime, is_locked, scope_review_required)
             VALUES ('sa-win-{key}', NULL, {equipment_sql}, 'shutdown', '2026-11-02T06:00:00Z',
                     '2026-11-02T18:00:00Z', 1, {})",
            i64::from(review)
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM planning_windows WHERE entity_sync_id = 'sa-win-{key}'"),
    )
    .await
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let admin = user(&db, "sa_admin").await;
    let site_admin = user(&db, "sa_site_admin").await;
    let viewer = user(&db, "sa_viewer").await;
    let admin_role = role(&db, "SA org admin", &[ORG_VIEW, ORG_ADMIN]).await;
    let site_admin_role = role(&db, "SA org admin site A", &[ORG_VIEW, ORG_ADMIN]).await;
    let viewer_role = role(&db, "SA org viewer", &[ORG_VIEW]).await;
    assign(&db, admin, admin_role, "tenant", None).await;
    assign(&db, site_admin, site_admin_role, "org_node", Some(t.site_a)).await;
    assign(&db, viewer, viewer_role, "tenant", None).await;

    let eq_a = equipment(&db, "A", t.site_a).await;
    let eq_b = equipment(&db, "B", t.site_b).await;
    let wo_none = work_order(&db, eq_a, admin, "Old order without site").await;
    let wo_closed_none = work_order(&db, eq_a, admin, "Old closed order without site").await;
    let wo_a = work_order(&db, eq_a, admin, "Order on site A").await;
    // Rows from before the node was required: stored without an org node; one is closed.
    exec(
        &db,
        &format!("UPDATE work_orders SET entity_id = NULL WHERE id IN ({wo_none}, {wo_closed_none})"),
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'closed')
              WHERE id = {wo_closed_none}"
        ),
    )
    .await;
    let di_none = request(&db, eq_a, t.site_a, admin, "Old request without site").await;
    exec(
        &db,
        &format!("UPDATE intervention_requests SET org_node_id = 0 WHERE id = {di_none}"),
    )
    .await;
    let win_review = window(&db, "review", None, true).await;
    let win_eq = window(&db, "eq", Some(eq_a), false).await;
    Fixture {
        db,
        t,
        admin,
        site_admin,
        viewer,
        eq_a,
        eq_b,
        wo_none,
        wo_closed_none,
        wo_a,
        di_none,
        win_review,
        win_eq,
    }
}

fn input(kind: NoSiteKind, ids: &[i64], node: i64, reason: &str) -> AssignSiteInput {
    AssignSiteInput {
        kind,
        record_ids: ids.to_vec(),
        org_node_id: node,
        reason: reason.to_string(),
    }
}

async fn opt_text(db: &DatabaseConnection, sql: &str) -> Option<String> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<Option<String>>(0)
        .expect("value")
}

async fn audit_count(db: &DatabaseConnection, target_type: &str, ids: &[i64]) -> i64 {
    let ids = ids.iter().map(|i| format!("'{i}'")).collect::<Vec<_>>().join(",");
    scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM audit_events WHERE action_code = '{}' AND target_type = '{target_type}'
               AND target_id IN ({ids})",
            site_assignment::AUDIT_ACTION
        ),
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Assigning a site
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn work_orders_without_site_get_the_site_with_one_audit_entry_each_whatever_their_status() {
    let f = fixture().await;
    let before = site_assignment::count_records_without_site(&f.db)
        .await
        .expect("counts");
    let version = |id: i64| format!("SELECT row_version FROM work_orders WHERE id = {id}");
    let status = |id: i64| format!("SELECT status_id FROM work_orders WHERE id = {id}");
    let (v_open, v_closed) = (
        scalar(&f.db, &version(f.wo_none)).await,
        scalar(&f.db, &version(f.wo_closed_none)).await,
    );
    let closed_status = scalar(&f.db, &status(f.wo_closed_none)).await;

    let out = site_assignment::assign_site(
        &f.db,
        f.admin,
        // A record named twice is given its site once.
        input(
            NoSiteKind::WorkOrder,
            &[f.wo_closed_none, f.wo_none, f.wo_none],
            f.t.site_a,
            &format!("  {REASON}  "),
        ),
    )
    .await
    .expect("assign");

    let mut expected = vec![f.wo_none, f.wo_closed_none];
    expected.sort_unstable();
    assert_eq!(out.assigned, expected);
    assert_eq!(
        out.counts.work_orders,
        before.work_orders - 2,
        "the 'to review' count goes down"
    );
    for id in [f.wo_none, f.wo_closed_none] {
        assert_eq!(
            scalar(&f.db, &format!("SELECT entity_id FROM work_orders WHERE id = {id}")).await,
            f.t.site_a
        );
        assert_eq!(
            scalar(
                &f.db,
                &format!("SELECT site_assigned_by_id FROM work_orders WHERE id = {id}")
            )
            .await,
            i64::from(f.admin)
        );
        assert_eq!(
            opt_text(
                &f.db,
                &format!("SELECT site_assign_reason FROM work_orders WHERE id = {id}")
            )
            .await,
            Some(REASON.to_string()),
            "the reason is kept, trimmed"
        );
        assert!(opt_text(
            &f.db,
            &format!("SELECT site_assigned_at FROM work_orders WHERE id = {id}")
        )
        .await
        .is_some());
    }
    assert_eq!(scalar(&f.db, &version(f.wo_none)).await, v_open + 1);
    assert_eq!(scalar(&f.db, &version(f.wo_closed_none)).await, v_closed + 1);
    assert_eq!(
        scalar(&f.db, &status(f.wo_closed_none)).await,
        closed_status,
        "a closed work order stays closed"
    );

    assert_eq!(
        audit_count(&f.db, "work_order", &expected).await,
        2,
        "one entry per record"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = '{}' AND target_type = 'work_order'
                   AND target_id = '{}' AND actor_id = {} AND json_extract(details_json, '$.site_before') IS NULL
                   AND json_extract(details_json, '$.site_after') = {} AND json_extract(details_json, '$.reason') = '{REASON}'",
                site_assignment::AUDIT_ACTION,
                f.wo_none,
                f.admin,
                f.t.site_a
            ),
        )
        .await,
        1,
        "the entry says who, from no site to the node, and why"
    );
}

#[tokio::test]
async fn a_record_that_has_a_site_refuses_the_whole_batch() {
    let f = fixture().await;
    let err = site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::WorkOrder, &[f.wo_none, f.wo_a], f.t.site_b, REASON),
    )
    .await
    .expect_err("a work order with a site");
    assert_eq!(code_of(&err), "SITE_ASSIGN_ALREADY_HAS_SITE");
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM work_orders WHERE id = {} AND entity_id IS NULL",
                f.wo_none
            )
        )
        .await,
        1,
        "nothing of the batch is applied"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT entity_id FROM work_orders WHERE id = {}", f.wo_a)
        )
        .await,
        f.t.site_a,
        "a site is never changed here"
    );
    assert_eq!(audit_count(&f.db, "work_order", &[f.wo_none, f.wo_a]).await, 0);

    let err = site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::WorkOrder, &[f.wo_none, 99_999_999], f.t.site_a, REASON),
    )
    .await
    .expect_err("an unknown work order");
    assert!(matches!(err, crate::errors::AppError::NotFound { .. }), "{err:?}");
}

#[tokio::test]
async fn only_a_plant_wide_organisation_administrator_assigns_a_site() {
    let f = fixture().await;
    let err = site_assignment::assign_site(
        &f.db,
        f.site_admin,
        input(NoSiteKind::WorkOrder, &[f.wo_none], f.t.site_a, REASON),
    )
    .await
    .expect_err("an administrator of site A only");
    assert_eq!(code_of(&err), "SCOPE_PLANT_WIDE_ONLY");
    let err = site_assignment::assign_site(
        &f.db,
        f.viewer,
        input(NoSiteKind::WorkOrder, &[f.wo_none], f.t.site_a, REASON),
    )
    .await
    .expect_err("no administration right");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM work_orders WHERE id = {} AND entity_id IS NULL",
                f.wo_none
            )
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_deleted_or_unknown_node_and_a_missing_reason_are_refused() {
    let f = fixture().await;
    let gone = crate::auth::scope_tests::node(&f.db, "SA-GONE", Some(f.t.plant)).await;
    exec(
        &f.db,
        &format!("UPDATE org_nodes SET deleted_at = '2026-01-02T00:00:00Z' WHERE id = {gone}"),
    )
    .await;
    for node in [gone, 99_999_999] {
        let err =
            site_assignment::assign_site(&f.db, f.admin, input(NoSiteKind::WorkOrder, &[f.wo_none], node, REASON))
                .await
                .expect_err("not a live node");
        assert_eq!(code_of(&err), "SITE_ASSIGN_NODE_INVALID", "node {node}");
    }
    for reason in ["", "   ", "too short"] {
        let err = site_assignment::assign_site(
            &f.db,
            f.admin,
            input(NoSiteKind::WorkOrder, &[f.wo_none], f.t.site_a, reason),
        )
        .await
        .expect_err("no reason");
        assert_eq!(code_of(&err), "SITE_ASSIGN_REASON_REQUIRED", "reason {reason:?}");
    }
    let err = site_assignment::assign_site(&f.db, f.admin, input(NoSiteKind::WorkOrder, &[], f.t.site_a, REASON))
        .await
        .expect_err("no record");
    assert_eq!(code_of(&err), "SITE_ASSIGN_NO_RECORDS");
    assert_eq!(audit_count(&f.db, "work_order", &[f.wo_none]).await, 0);
}

#[tokio::test]
async fn a_request_stored_without_site_gets_the_site() {
    let f = fixture().await;
    let before = site_assignment::count_records_without_site(&f.db)
        .await
        .expect("counts");
    let out = site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::Request, &[f.di_none], f.t.line_a1, REASON),
    )
    .await
    .expect("assign");
    assert_eq!(out.counts.requests, before.requests - 1);
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT org_node_id FROM intervention_requests WHERE id = {}", f.di_none)
        )
        .await,
        f.t.line_a1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = '{}' AND target_type = 'intervention_request'
                   AND target_id = '{}' AND json_extract(details_json, '$.site_before') = 0",
                site_assignment::AUDIT_ACTION,
                f.di_none
            ),
        )
        .await,
        1,
        "the entry records the stored 0 as the site before"
    );
    let err = site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::Request, &[f.di_none], f.t.site_b, REASON),
    )
    .await
    .expect_err("it has a site now");
    assert_eq!(code_of(&err), "SITE_ASSIGN_ALREADY_HAS_SITE");
}

#[tokio::test]
async fn a_planning_window_without_site_gets_a_site_holding_its_equipment() {
    let f = fixture().await;
    let before = site_assignment::count_records_without_site(&f.db)
        .await
        .expect("counts");
    let out = site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::PlanningWindow, &[f.win_review], f.t.site_b, REASON),
    )
    .await
    .expect("assign");
    assert_eq!(
        out.counts.planning_windows_to_review,
        before.planning_windows_to_review - 1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM planning_windows WHERE id = {} AND entity_id = {} AND scope_review_required = 0",
                f.win_review, f.t.site_b
            ),
        )
        .await,
        1,
        "the window has its site and is no longer to re-enter"
    );
    assert_eq!(audit_count(&f.db, "planning_window", &[f.win_review]).await, 1);

    // The window's equipment is installed on site A: site B is refused, as in the window dialog.
    let err = site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::PlanningWindow, &[f.win_eq], f.t.site_b, REASON),
    )
    .await
    .expect_err("equipment not on site B");
    assert_eq!(code_of(&err), "PLANNING_WINDOW_EQUIPMENT_NOT_ON_SITE");
    site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::PlanningWindow, &[f.win_eq], f.t.site_a, REASON),
    )
    .await
    .expect("site A holds its equipment");
}

#[tokio::test]
async fn the_list_counts_every_kind_and_suggests_the_equipment_site() {
    let f = fixture().await;
    let data = site_assignment::list_records_without_site(&f.db, NoSiteKind::WorkOrder)
        .await
        .expect("list");
    let listed: Vec<i64> = data.records.iter().map(|r| r.id).collect();
    assert!(listed.contains(&f.wo_none) && listed.contains(&f.wo_closed_none));
    assert!(!listed.contains(&f.wo_a), "a work order with a site is not listed");
    let row = data.records.iter().find(|r| r.id == f.wo_none).expect("listed");
    assert_eq!(row.suggested_node_id, Some(f.t.site_a), "its equipment's location");
    assert_eq!(row.equipment_id, Some(f.eq_a));
    assert!(row.to_review);
    assert!(data.nodes.iter().any(|n| n.id == f.t.site_b));
    assert_eq!(
        data.counts.work_orders,
        scalar(
            &f.db,
            "SELECT COUNT(*) FROM work_orders WHERE entity_id IS NULL OR entity_id <= 0"
        )
        .await
    );
    assert!(data.counts.requests >= 1);
    assert_eq!(
        data.counts.to_review(),
        data.counts.work_orders + data.counts.requests + data.counts.planning_windows_to_review
    );

    let windows = site_assignment::list_records_of_kind(&f.db, NoSiteKind::PlanningWindow)
        .await
        .expect("windows");
    let review = windows.iter().find(|r| r.id == f.win_review).expect("listed");
    let plant_wide = windows.iter().find(|r| r.id == f.win_eq).expect("listed");
    assert!(review.to_review, "a window that lost its site is to review");
    assert!(!plant_wide.to_review, "a plant-wide window is valid");
    assert_eq!(plant_wide.suggested_node_id, Some(f.t.site_a));
    assert_eq!(review.suggested_node_id, None, "no equipment, no suggestion");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Arrival by synchronisation
// ═══════════════════════════════════════════════════════════════════════════════

fn payload(fields: &[(&str, Value)]) -> Map<String, Value> {
    fields.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A received assignment of `node_sync` by `by_sync`, the site under `site_key`.
fn assignment(site_key: &str, node_sync: &str, by_sync: &str, at: &str, reason: &str) -> Map<String, Value> {
    payload(&[
        (site_key, json!(node_sync)),
        ("site_assigned_by_sync_id", json!(by_sync)),
        ("site_assigned_at", json!(at)),
        ("site_assign_reason", json!(reason)),
    ])
}

async fn sync_id(db: &DatabaseConnection, table: &str, id: i64) -> String {
    opt_text(db, &format!("SELECT entity_sync_id FROM {table} WHERE id = {id}"))
        .await
        .expect("sync id")
}

fn conflict_code(verdict: Option<InboundVerdict>) -> &'static str {
    match verdict {
        Some(InboundVerdict::Conflict { code, .. }) => code,
        other => panic!("expected a conflict, got {other:?}"),
    }
}

fn waits(verdict: &Option<InboundVerdict>) -> bool {
    matches!(verdict, Some(InboundVerdict::Defer(_)))
}

const ADMIN_SYNC: &str = "scope-user-sa_admin";
const SITE_A_SYNC: &str = "scope-A";

#[tokio::test]
async fn a_received_assignment_is_accepted_when_its_author_may_and_the_record_has_no_site_here() {
    let f = fixture().await;
    let wo = sync_id(&f.db, "work_orders", f.wo_closed_none).await;
    let row = assignment("entity_sync_id", SITE_A_SYNC, ADMIN_SYNC, &now(), REASON);
    let verdict = inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
        .await
        .expect("check");
    assert_eq!(verdict, None, "accepted, closed work order included");

    let di = sync_id(&f.db, "intervention_requests", f.di_none).await;
    let row = assignment("org_node_sync_id", SITE_A_SYNC, ADMIN_SYNC, &now(), REASON);
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::Request, &di, &row)
            .await
            .expect("check"),
        None
    );

    let row = assignment("entity_sync_id", SITE_A_SYNC, ADMIN_SYNC, &now(), REASON);
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::PlanningWindow, "sa-win-review", &row)
            .await
            .expect("check"),
        None
    );

    // A record not on this PC yet is new here: nothing to compare.
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, "not-here", &row)
            .await
            .expect("check"),
        None
    );
}

#[tokio::test]
async fn a_received_assignment_is_a_conflict_when_the_record_has_a_site_here() {
    let f = fixture().await;
    let wo = sync_id(&f.db, "work_orders", f.wo_a).await;
    let row = assignment("entity_sync_id", "scope-B", ADMIN_SYNC, &now(), REASON);
    assert_eq!(
        conflict_code(
            inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
                .await
                .expect("check")
        ),
        "SITE_ASSIGNMENT_SITE_NOT_EMPTY"
    );
}

#[tokio::test]
async fn a_received_assignment_is_a_conflict_when_its_author_lacks_the_plant_wide_right_here() {
    let f = fixture().await;
    let wo = sync_id(&f.db, "work_orders", f.wo_none).await;
    for author in ["scope-user-sa_site_admin", "scope-user-sa_viewer"] {
        let row = assignment("entity_sync_id", SITE_A_SYNC, author, &now(), REASON);
        assert_eq!(
            conflict_code(
                inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
                    .await
                    .expect("check")
            ),
            "SITE_ASSIGNMENT_NOT_ALLOWED",
            "{author}"
        );
    }
}

#[tokio::test]
async fn a_received_assignment_waits_for_its_author_and_site_and_refuses_a_deleted_site() {
    let f = fixture().await;
    let wo = sync_id(&f.db, "work_orders", f.wo_none).await;
    let row = assignment("entity_sync_id", SITE_A_SYNC, "user-not-here-yet", &now(), REASON);
    assert!(waits(
        &inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
            .await
            .expect("check")
    ));
    let row = assignment("entity_sync_id", "node-not-here-yet", ADMIN_SYNC, &now(), REASON);
    assert!(waits(
        &inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
            .await
            .expect("check")
    ));
    let gone = crate::auth::scope_tests::node(&f.db, "SA-GONE", Some(f.t.plant)).await;
    exec(
        &f.db,
        &format!("UPDATE org_nodes SET deleted_at = '2026-01-02T00:00:00Z' WHERE id = {gone}"),
    )
    .await;
    let row = assignment("entity_sync_id", "scope-SA-GONE", ADMIN_SYNC, &now(), REASON);
    assert_eq!(
        conflict_code(
            inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
                .await
                .expect("check")
        ),
        "SITE_ASSIGNMENT_NODE_INVALID"
    );
}

#[tokio::test]
async fn a_received_assignment_without_reason_or_dated_in_the_future_is_a_conflict() {
    let f = fixture().await;
    let wo = sync_id(&f.db, "work_orders", f.wo_none).await;
    let future = (chrono::Utc::now() + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    for row in [
        assignment("entity_sync_id", SITE_A_SYNC, ADMIN_SYNC, &now(), "short"),
        assignment("entity_sync_id", SITE_A_SYNC, ADMIN_SYNC, &future, REASON),
        assignment("entity_sync_id", SITE_A_SYNC, ADMIN_SYNC, "not a time", REASON),
        payload(&[
            ("entity_sync_id", json!(SITE_A_SYNC)),
            ("site_assigned_by_sync_id", json!(ADMIN_SYNC)),
        ]),
        assignment("entity_sync_id", "", ADMIN_SYNC, &now(), REASON),
    ] {
        assert_eq!(
            conflict_code(
                inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
                    .await
                    .expect("check")
            ),
            "SITE_ASSIGNMENT_INCOMPLETE",
            "{row:?}"
        );
    }
}

#[tokio::test]
async fn a_site_received_without_assignment_on_a_record_without_site_is_a_conflict() {
    let f = fixture().await;
    let wo = sync_id(&f.db, "work_orders", f.wo_none).await;
    let row = payload(&[("entity_sync_id", json!(SITE_A_SYNC))]);
    assert_eq!(
        conflict_code(
            inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
                .await
                .expect("check")
        ),
        "SITE_SET_WITHOUT_ASSIGNMENT"
    );
    // A work order moves with its new equipment (the edit on the other PC): not judged here.
    let row = payload(&[
        ("entity_sync_id", json!("scope-B")),
        ("equipment_sync_id", json!("sa-eq-B")),
    ]);
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
            .await
            .expect("check"),
        None
    );
    let _ = f.eq_b;

    let di = sync_id(&f.db, "intervention_requests", f.di_none).await;
    let row = payload(&[("org_node_sync_id", json!(SITE_A_SYNC))]);
    assert_eq!(
        conflict_code(
            inbound_site_assignment_check(&f.db, NoSiteKind::Request, &di, &row)
                .await
                .expect("check")
        ),
        "SITE_SET_WITHOUT_ASSIGNMENT"
    );

    // A planner re-enters a window's site in the window dialog: not judged here.
    let row = payload(&[("entity_sync_id", json!(SITE_A_SYNC))]);
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::PlanningWindow, "sa-win-review", &row)
            .await
            .expect("check"),
        None
    );
}

#[tokio::test]
async fn the_assignment_recorded_here_is_never_rewritten_by_sync() {
    let f = fixture().await;
    site_assignment::assign_site(
        &f.db,
        f.admin,
        input(NoSiteKind::WorkOrder, &[f.wo_none], f.t.site_a, REASON),
    )
    .await
    .expect("assign");
    let wo = sync_id(&f.db, "work_orders", f.wo_none).await;
    let at = opt_text(
        &f.db,
        &format!("SELECT site_assigned_at FROM work_orders WHERE id = {}", f.wo_none),
    )
    .await
    .expect("at");

    // The same trace (the row as this PC sent it, or with other fields changed): accepted.
    let same = assignment("entity_sync_id", SITE_A_SYNC, ADMIN_SYNC, &at, REASON);
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &same)
            .await
            .expect("check"),
        None
    );
    let untouched = payload(&[("title", json!("Renamed"))]);
    assert_eq!(
        inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &untouched)
            .await
            .expect("check"),
        None
    );
    for row in [
        assignment(
            "entity_sync_id",
            SITE_A_SYNC,
            ADMIN_SYNC,
            &at,
            "Another reason, rewritten",
        ),
        payload(&[
            ("site_assigned_by_sync_id", Value::Null),
            ("site_assigned_at", Value::Null),
            ("site_assign_reason", Value::Null),
        ]),
    ] {
        assert_eq!(
            conflict_code(
                inbound_site_assignment_check(&f.db, NoSiteKind::WorkOrder, &wo, &row)
                    .await
                    .expect("check")
            ),
            "SITE_ASSIGNMENT_TRACE_CHANGED",
            "{row:?}"
        );
    }
}
