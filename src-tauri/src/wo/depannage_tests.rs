//! Dépannage path (`wo::depannage`, CMMS Advisor decision 2026-11-04): one transaction creates a
//! corrective work order and starts it through the state machine, the intermediate moves recorded
//! as automatic with no approver; the rights are site-scoped (`wo::access::check_depannage_at`);
//! every refusal (facts, retired equipment) leaves nothing behind. A permit needed is never a
//! refusal (CMMS Advisor 2026-11-05): the work order waits, ready, for the normal start.

use chrono::Duration;
use sea_orm::DatabaseConnection;

use crate::auth::scope::{self, AccessMap};
use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::errors::AppError;
use crate::rbac::permissions::{OT_CREATE, OT_EDIT, OT_EMERGENCY_ASSIGN, OT_VIEW};
use crate::wo::access;
use crate::wo::depannage::{
    self, DepannageOutcome, WoBreakdownAtInput, WoDepannageInput, AUTO_NOTE, AUTO_REASON_CODE,
    CODE_BREAKDOWN_AT_FUTURE, CODE_BREAKDOWN_AT_REQUIRED, CODE_MACHINE_STOPPED_REQUIRED, CODE_NOT_A_DEPANNAGE,
    NOTIFICATION_CATEGORY, PERMIT_NEEDED_CATEGORY, WAITING_MESSAGE_KEY,
};
use crate::wo::time::stamp;

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// ot.create + ot.edit on site A.
    tech: i32,
    /// ot.create + ot.edit + ot.emergency_assign on site A.
    lead: i32,
    /// ot.view only on site A (the person a lead sends).
    colleague: i32,
    /// ot.create only on site A (no ot.edit).
    creator_only: i32,
    /// Equipment installed on site A and on site B.
    eq_a: i64,
    eq_b: i64,
}

async fn equipment(db: &DatabaseConnection, code: &str, node: i64) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('dep-eq-{code}', 'DEP-{code}', 'Equipment {code}', 'active_in_service', {node},
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'dep-eq-{code}'"),
    )
    .await
}

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let tech = user(&db, "dep_tech").await;
    let lead = user(&db, "dep_lead").await;
    let colleague = user(&db, "dep_colleague").await;
    let creator_only = user(&db, "dep_creator_only").await;
    let tech_role = role(&db, "Dépannage tech A", &[OT_VIEW, OT_CREATE, OT_EDIT]).await;
    let lead_role = role(
        &db,
        "Dépannage lead A",
        &[OT_VIEW, OT_CREATE, OT_EDIT, OT_EMERGENCY_ASSIGN],
    )
    .await;
    let viewer_role = role(&db, "Dépannage viewer A", &[OT_VIEW]).await;
    let creator_role = role(&db, "Dépannage creator A", &[OT_VIEW, OT_CREATE]).await;
    assign(&db, tech, tech_role, "org_node", Some(t.site_a)).await;
    assign(&db, lead, lead_role, "org_node", Some(t.site_a)).await;
    assign(&db, colleague, viewer_role, "org_node", Some(t.site_a)).await;
    assign(&db, creator_only, creator_role, "org_node", Some(t.site_a)).await;
    let eq_a = equipment(&db, "A", t.site_a).await;
    let eq_b = equipment(&db, "B", t.site_b).await;
    Fixture {
        db,
        t,
        tech,
        lead,
        colleague,
        creator_only,
        eq_a,
        eq_b,
    }
}

async fn map_of(db: &DatabaseConnection, user_id: i32) -> AccessMap {
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    scope::access_map_cached(db, &cache, user_id).await.expect("access map")
}

fn now_minus(d: Duration) -> String {
    stamp(crate::wo::time::now_utc_second() - d)
}

fn input(f: &Fixture, actor: i32) -> WoDepannageInput {
    WoDepannageInput {
        equipment_id: f.eq_a,
        type_code: None,
        urgency_code: Some("emergency".into()),
        subtype_code: None,
        title: "Convoyeur bloqué".into(),
        description: Some("Bourrage en sortie de ligne".into()),
        breakdown_at: Some(now_minus(Duration::minutes(20))),
        machine_stopped: Some(true),
        assignee_id: None,
        requires_permit: None,
        actor_id: i64::from(actor),
        permit_drafting: None,
    }
}

async fn work_orders(db: &DatabaseConnection) -> i64 {
    scalar(db, "SELECT COUNT(*) FROM work_orders").await
}

async fn transitions(db: &DatabaseConnection) -> i64 {
    scalar(db, "SELECT COUNT(*) FROM wo_state_transition_log").await
}

fn refused_with(err: &AppError, code: &str) {
    assert!(format!("{err:?}").contains(code), "expected {code}, got {err:?}");
}

#[tokio::test]
async fn a_self_assigned_depannage_is_created_and_started_with_automatic_unapproved_moves() {
    let f = fixture().await;
    access::check_depannage_at(&f.db, &map_of(&f.db, f.tech).await, None, Some(f.eq_a), false)
        .await
        .expect("ot.create + ot.edit on site A: may start a dépannage for themself");

    let mut i = input(&f, f.tech);
    i.machine_stopped = Some(false);
    let breakdown = i.breakdown_at.clone().expect("breakdown");
    let done = depannage::start_depannage(&f.db, i).await.expect("dépannage");
    let wo = &done.wo;
    assert!(!done.breakdown_old_warning);
    assert_eq!(wo.status_code.as_deref(), Some("in_progress"));
    assert!(wo.is_depannage);
    assert_eq!(wo.breakdown_at.as_deref(), Some(breakdown.as_str()));
    assert_eq!(wo.machine_stopped, Some(false));
    assert_eq!(wo.primary_responsible_id, Some(i64::from(f.tech)));
    assert_eq!(wo.entity_id, Some(f.t.site_a));
    assert!(wo.actual_start.is_some(), "started");
    assert!(wo.planned_start.is_none() && wo.planned_end.is_none() && wo.expected_duration_hours.is_none());
    assert!(wo.validated_by_id.is_none() && wo.validated_at.is_none() && wo.validation_auto_approved.is_none());
    // Its type is of the corrective nature (immediate, the default).
    let nature = crate::wo::types::work_order_nature(&f.db, wo.id).await.expect("nature");
    assert!(nature.is_some_and(crate::wo::types::MaintenanceNature::is_corrective));

    // No approval recorded: none happened.
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM work_orders WHERE id = {} AND approver_id IS NULL \
                   AND planning_approved_by_id IS NULL AND planning_approved_at IS NULL",
                wo.id
            )
        )
        .await,
        1
    );
    // The creation, then three automatic moves by the user, marked.
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {}", wo.id)
        )
        .await,
        4
    );
    for (from, to, action) in [
        ("draft", "planning", "submit"),
        ("planning", "ready", "mark_ready"),
        ("ready", "in_progress", "start"),
    ] {
        assert_eq!(
            scalar(
                &f.db,
                &format!(
                    "SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {} AND from_status = '{from}' \
                       AND to_status = '{to}' AND action = '{action}' AND actor_id = {} \
                       AND reason_code = '{AUTO_REASON_CODE}' AND notes = '{AUTO_NOTE}'",
                    wo.id, f.tech
                )
            )
            .await,
            1,
            "{from} → {to} recorded as automatic"
        );
    }
    // Unscheduled: no schedule commitment, so no weekly schedule (frozen or not) holds it.
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM schedule_commitments WHERE source_type = 'work_order' AND source_id = {}",
                wo.id
            )
        )
        .await,
        0
    );
    // The planners of the site and the node's responsibles are told.
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM notification_events WHERE category_code = '{NOTIFICATION_CATEGORY}' \
                   AND source_module = 'work_orders' AND source_record_id = '{}'",
                wo.id
            )
        )
        .await,
        1
    );
}

#[tokio::test]
async fn assigning_someone_else_needs_the_emergency_assign_right_at_the_site() {
    let f = fixture().await;
    let err = access::check_depannage_at(&f.db, &map_of(&f.db, f.tech).await, None, Some(f.eq_a), true)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");

    access::check_depannage_at(&f.db, &map_of(&f.db, f.lead).await, None, Some(f.eq_a), true)
        .await
        .expect("ot.emergency_assign on site A");
    let mut i = input(&f, f.lead);
    i.assignee_id = Some(i64::from(f.colleague));
    let done = depannage::start_depannage(&f.db, i)
        .await
        .expect("dépannage for a colleague");
    assert_eq!(done.wo.primary_responsible_id, Some(i64::from(f.colleague)));
    assert_eq!(done.wo.status_code.as_deref(), Some("in_progress"));
}

#[tokio::test]
async fn rights_are_checked_at_the_equipments_site() {
    let f = fixture().await;
    let tech = map_of(&f.db, f.tech).await;
    let err = access::check_depannage_at(&f.db, &tech, None, Some(f.eq_b), false)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "SCOPE_OUTSIDE_SITE", "equipment of another site");
    // Creating is not enough: starting needs ot.edit.
    let err = access::check_depannage_at(&f.db, &map_of(&f.db, f.creator_only).await, None, Some(f.eq_a), false)
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
}

#[tokio::test]
async fn missing_or_future_facts_are_refused_and_nothing_is_written() {
    let f = fixture().await;
    let (wos, logs) = (work_orders(&f.db).await, transitions(&f.db).await);

    let mut i = input(&f, f.tech);
    i.machine_stopped = None;
    refused_with(
        &depannage::start_depannage(&f.db, i).await.unwrap_err(),
        CODE_MACHINE_STOPPED_REQUIRED,
    );

    let mut i = input(&f, f.tech);
    i.breakdown_at = None;
    refused_with(
        &depannage::start_depannage(&f.db, i).await.unwrap_err(),
        CODE_BREAKDOWN_AT_REQUIRED,
    );

    let mut i = input(&f, f.tech);
    i.breakdown_at = Some(now_minus(Duration::hours(-1)));
    refused_with(
        &depannage::start_depannage(&f.db, i).await.unwrap_err(),
        CODE_BREAKDOWN_AT_FUTURE,
    );
    assert_eq!(work_orders(&f.db).await, wos);
    assert_eq!(transitions(&f.db).await, logs);

    // A clock a little ahead is tolerated.
    let mut i = input(&f, f.tech);
    i.breakdown_at = Some(now_minus(Duration::minutes(-2)));
    depannage::start_depannage(&f.db, i)
        .await
        .expect("small clock difference");
}

#[tokio::test]
async fn an_old_breakdown_is_accepted_with_a_warning() {
    let f = fixture().await;
    let mut i = input(&f, f.tech);
    i.breakdown_at = Some(now_minus(Duration::days(8)));
    let done = depannage::start_depannage(&f.db, i).await.expect("old breakdown");
    assert!(done.breakdown_old_warning);
    assert_eq!(done.wo.status_code.as_deref(), Some("in_progress"));

    let mut i = input(&f, f.tech);
    i.breakdown_at = Some(now_minus(Duration::days(6)));
    assert!(
        !depannage::start_depannage(&f.db, i)
            .await
            .expect("recent")
            .breakdown_old_warning
    );
}

#[tokio::test]
async fn a_retired_equipment_is_refused_and_nothing_is_written() {
    let f = fixture().await;
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&f.db)
        .await
        .expect("status tags");
    exec(
        &f.db,
        &format!(
            "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED' WHERE id = {}",
            f.eq_a
        ),
    )
    .await;
    let (wos, logs) = (work_orders(&f.db).await, transitions(&f.db).await);
    let err = depannage::start_depannage(&f.db, input(&f, f.tech)).await.unwrap_err();
    refused_with(&err, "GATE_WO_EQUIPMENT_RETIRED");
    assert_eq!(work_orders(&f.db).await, wos);
    assert_eq!(transitions(&f.db).await, logs);
}

// ── A permit needed (CMMS Advisor decision 2026-11-05): never refused, ready and waiting ──────────

/// An account holding `permissions` on the org node `site` only.
async fn holder(db: &DatabaseConnection, username: &str, site: i64, permissions: &[&str]) -> i32 {
    let u = user(db, username).await;
    let r = role(db, &format!("Dépannage {username}"), permissions).await;
    assign(db, u, r, "org_node", Some(site)).await;
    u
}

/// A plant permit rule on the dépannage's own work type (the plant's type for immediate corrective
/// work), calling for `permit_type`.
async fn rule_on_depannage_type(db: &DatabaseConnection, code: &str, permit_type: &str) {
    let work_type =
        crate::wo::types::default_type_for_nature(db, crate::wo::types::MaintenanceNature::CorrectiveImmediate)
            .await
            .expect("dépannage type")
            .code;
    crate::permit::requirement_rules::add_published_rule(
        db,
        code,
        serde_json::json!({ "work_type_code": work_type, "permit_type_code": permit_type }),
    )
    .await;
}

async fn events_of(db: &DatabaseConnection, category: &str, wo_id: i64) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM notification_events WHERE category_code = '{category}' \
               AND source_module = 'work_orders' AND source_record_id = '{wo_id}'"
        ),
    )
    .await
}

async fn told(db: &DatabaseConnection, category: &str, wo_id: i64, user: i32) -> bool {
    scalar(
        db,
        &format!(
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
              WHERE e.category_code = '{category}' AND e.source_record_id = '{wo_id}' AND n.recipient_user_id = {user}"
        ),
    )
    .await
        > 0
}

/// Moves a permit through review, approval and issue to active, as the permit screens do (four
/// eyes: the approver is neither its author nor its submitter).
async fn make_live(db: &DatabaseConnection, permit_id: i64, approver: i32) {
    let admin = scalar(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    for (status, actor) in [
        ("pending_review", admin),
        ("approved", i64::from(approver)),
        ("issued", admin),
        ("active", admin),
    ] {
        let wp = crate::permit::queries::get_work_permit(db, permit_id)
            .await
            .expect("get")
            .expect("permit");
        crate::permit::queries::set_work_permit_status(
            db,
            crate::permit::domain::WorkPermitStatusInput {
                id: permit_id,
                status: status.into(),
                expected_row_version: wp.row_version,
                reason: None,
            },
            actor,
        )
        .await
        .unwrap_or_else(|e| panic!("-> {status}: {e:?}"));
    }
}

async fn start_normally(
    db: &DatabaseConnection,
    wo_id: i64,
    actor: i32,
) -> Result<crate::wo::domain::WorkOrder, AppError> {
    let rv = crate::wo::queries::get_work_order(db, wo_id)
        .await
        .expect("get")
        .expect("wo")
        .row_version;
    crate::wo::execution::start_wo(
        db,
        crate::wo::execution::WoStartInput {
            wo_id,
            actor_id: i64::from(actor),
            expected_row_version: rv,
        },
    )
    .await
}

/// The work order is ready, assigned, not started, carries every dépannage fact, and no approval of
/// any kind was recorded (none happened).
async fn assert_waiting(f: &Fixture, done: &depannage::WoDepannageStartResult, assignee: i32) {
    let wo = &done.wo;
    assert_eq!(done.outcome, DepannageOutcome::WaitingForPermit);
    assert_eq!(wo.status_code.as_deref(), Some("ready"), "prête, not started");
    assert!(wo.actual_start.is_none(), "not started");
    assert!(wo.is_depannage && wo.breakdown_at.is_some() && wo.machine_stopped == Some(true));
    assert_eq!(wo.primary_responsible_id, Some(i64::from(assignee)), "assigned");
    assert!(wo.validated_by_id.is_none() && wo.validated_at.is_none() && wo.validation_auto_approved.is_none());
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM work_orders WHERE id = {} AND approver_id IS NULL \
                   AND planning_approved_by_id IS NULL AND planning_approved_at IS NULL",
                wo.id
            )
        )
        .await,
        1
    );
    // Created, then draft → planning → ready, automatic; no start.
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {}", wo.id)
        )
        .await,
        3
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {} AND to_status = 'in_progress'",
                wo.id
            )
        )
        .await,
        0
    );
    // The planners are told it waits; nobody is told it started.
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM notification_events WHERE category_code = '{NOTIFICATION_CATEGORY}' \
                   AND source_record_id = '{}' AND message_key = '{WAITING_MESSAGE_KEY}'",
                wo.id
            )
        )
        .await,
        1
    );
    assert_eq!(events_of(&f.db, NOTIFICATION_CATEGORY, wo.id).await, 1);
    // The permit issuers of the site are told at once, in every case.
    assert_eq!(events_of(&f.db, PERMIT_NEEDED_CATEGORY, wo.id).await, 1);
}

/// A plant rule requires a permit: the dépannage is not refused, it is ready and waits for it; the
/// required type is listed (the one the start gate reads); without the permit-issue right no draft
/// is written; the issuers of the site are told (not those of another site); a start before the
/// permit is live is refused by the normal gate; once the permit is live, the normal start succeeds
/// and is logged as any start.
#[tokio::test]
async fn a_permit_needed_leaves_the_depannage_ready_and_waiting_without_a_draft_when_the_user_cannot_issue() {
    let f = fixture().await;
    rule_on_depannage_type(&f.db, "DEP_HOT_WORK", "hot_work").await;
    let issuer_a = holder(
        &f.db,
        "dep_issuer_a",
        f.t.site_a,
        &[crate::rbac::permissions::PTW_ISSUE],
    )
    .await;
    let issuer_b = holder(
        &f.db,
        "dep_issuer_b",
        f.t.site_b,
        &[crate::rbac::permissions::PTW_ISSUE],
    )
    .await;
    let permits_before = scalar(&f.db, "SELECT COUNT(*) FROM work_permits").await;

    // The tech holds no permit right: their access map is given, the site check refuses the draft.
    let mut i = input(&f, f.tech);
    i.permit_drafting = Some(map_of(&f.db, f.tech).await);
    let done = depannage::start_depannage(&f.db, i)
        .await
        .expect("never refused for the permit");
    assert_waiting(&f, &done, f.tech).await;
    assert_eq!(done.required_permit_types, vec!["hot_work".to_string()]);
    assert!(done.permit_draft_ids.is_empty(), "no right: no draft");
    assert_eq!(scalar(&f.db, "SELECT COUNT(*) FROM work_permits").await, permits_before);
    let wo_id = done.wo.id;
    assert!(
        told(&f.db, PERMIT_NEEDED_CATEGORY, wo_id, issuer_a).await,
        "issuer of the site"
    );
    assert!(
        !told(&f.db, PERMIT_NEEDED_CATEGORY, wo_id, issuer_b).await,
        "not another site's"
    );
    // Every recipient (the site's issuer, and the administrators who hold every right) gets
    // "Créer le permis" for the work order, critical.
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
                  WHERE e.category_code = '{PERMIT_NEEDED_CATEGORY}' AND e.source_record_id = '{wo_id}' \
                    AND (n.action_url IS NOT '/permits?createForWo={wo_id}' OR e.severity <> 'critical')"
            )
        )
        .await,
        0
    );

    // Started before a permit is live: the normal gate refuses it.
    let err = start_normally(&f.db, wo_id, f.tech).await.unwrap_err();
    refused_with(&err, "PERMIT_REQUIRED_TO_START");

    // The issuer writes the permit by hand; once live, the normal start succeeds.
    let permit = crate::permit::queries::create_work_permit(
        &f.db,
        crate::permit::domain::WorkPermitCreateInput {
            permit_type_id: scalar(&f.db, "SELECT id FROM permit_types WHERE code = 'hot_work'").await,
            asset_id: None,
            entity_id: None,
            linked_work_order_id: Some(wo_id),
            expires_at: Some(stamp(crate::wo::time::now_utc_second() + Duration::hours(6))),
        },
        i64::from(issuer_a),
    )
    .await
    .expect("permit");
    make_live(&f.db, permit.id, f.lead).await;
    let started = start_normally(&f.db, wo_id, f.tech).await.expect("normal start");
    assert_eq!(started.status_code.as_deref(), Some("in_progress"));
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {wo_id} AND from_status = 'ready' \
                   AND to_status = 'in_progress' AND action = 'start' AND actor_id = {} AND reason_code IS NULL",
                f.tech
            )
        )
        .await,
        1,
        "logged as any start, not as automatic"
    );
}

/// The user holds the permit-issue right at the site: one draft per required type, linked to the
/// work order and its equipment, written by them, in draft (no review, no approval); the issuers'
/// notice links to it. The draft made live, the normal start succeeds; before, it is refused.
#[tokio::test]
async fn a_user_who_may_issue_permits_gets_a_draft_per_required_type() {
    let f = fixture().await;
    rule_on_depannage_type(&f.db, "DEP_HOT_WORK", "hot_work").await;
    rule_on_depannage_type(&f.db, "DEP_COLD_WORK", "cold_work").await;
    let issuing_tech = holder(
        &f.db,
        "dep_issuing_tech",
        f.t.site_a,
        &[OT_VIEW, OT_CREATE, OT_EDIT, crate::rbac::permissions::PTW_ISSUE],
    )
    .await;

    let mut i = input(&f, issuing_tech);
    i.permit_drafting = Some(map_of(&f.db, issuing_tech).await);
    let done = depannage::start_depannage(&f.db, i).await.expect("dépannage");
    assert_waiting(&f, &done, issuing_tech).await;
    let wo_id = done.wo.id;
    // Every type the start gate requires is listed: the work order's plant rule type (one rule per
    // work order until the multi-permit model, L4-10b).
    let gate = crate::permit::wo_gate::permit_requirement(&f.db, wo_id)
        .await
        .expect("requirement");
    assert!(gate.required);
    assert_eq!(done.required_permit_types, gate.permit_types);
    assert_eq!(done.required_permit_types.len(), 1);
    assert_eq!(
        done.wo.permit_rule_permit_type.as_ref(),
        done.required_permit_types.first()
    );

    assert_eq!(
        done.permit_draft_ids.len(),
        done.required_permit_types.len(),
        "one draft per type"
    );
    for (draft_id, permit_type) in done.permit_draft_ids.iter().zip(&done.required_permit_types) {
        let draft = crate::permit::queries::get_work_permit(&f.db, *draft_id)
            .await
            .expect("get")
            .expect("draft");
        assert_eq!(draft.status, "draft");
        assert_eq!(draft.permit_type_code.as_ref(), Some(permit_type));
        assert_eq!(draft.linked_work_order_id, Some(wo_id));
        assert_eq!(draft.asset_id, f.eq_a);
        assert_eq!(draft.entity_id, f.t.site_a);
        assert_eq!(draft.created_by_id, Some(i64::from(issuing_tech)));
        assert!(
            draft.submitted_by_id.is_none() && draft.approved_by_id.is_none(),
            "no fake approval"
        );
        assert!(draft.expires_at.is_some());
    }
    let first = done.permit_draft_ids[0];
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
                  WHERE e.category_code = '{PERMIT_NEEDED_CATEGORY}' AND e.source_record_id = '{wo_id}' \
                    AND n.action_url = '/permits?permit={first}'"
            )
        )
        .await
            > 0,
        true,
        "the issuers' notice opens the draft"
    );

    // A draft is not a live permit: the normal gate refuses the start.
    let err = start_normally(&f.db, wo_id, issuing_tech).await.unwrap_err();
    refused_with(&err, "PERMIT_NOT_ACTIVE");
    for draft in &done.permit_draft_ids {
        make_live(&f.db, *draft, f.lead).await;
    }
    let started = start_normally(&f.db, wo_id, issuing_tech).await.expect("normal start");
    assert_eq!(started.status_code.as_deref(), Some("in_progress"));
}

/// A permit required by hand with no rule naming a type: waiting, no type listed, no draft (the
/// issuer chooses the type), the issuers told; with no permit required it starts as before.
#[tokio::test]
async fn a_permit_required_without_a_type_waits_with_no_draft() {
    let f = fixture().await;
    let issuing_tech = holder(
        &f.db,
        "dep_issuing_tech",
        f.t.site_a,
        &[OT_VIEW, OT_CREATE, OT_EDIT, crate::rbac::permissions::PTW_ISSUE],
    )
    .await;
    let (wos, permits) = (
        work_orders(&f.db).await,
        scalar(&f.db, "SELECT COUNT(*) FROM work_permits").await,
    );
    let mut i = input(&f, issuing_tech);
    i.requires_permit = Some(true);
    i.permit_drafting = Some(map_of(&f.db, issuing_tech).await);
    let done = depannage::start_depannage(&f.db, i)
        .await
        .expect("never refused for the permit");
    assert_eq!(work_orders(&f.db).await, wos + 1, "the breakdown report is kept");
    assert_waiting(&f, &done, issuing_tech).await;
    assert!(done.required_permit_types.is_empty());
    assert!(done.permit_draft_ids.is_empty());
    assert_eq!(scalar(&f.db, "SELECT COUNT(*) FROM work_permits").await, permits);

    let mut i = input(&f, issuing_tech);
    i.permit_drafting = Some(map_of(&f.db, issuing_tech).await);
    let done = depannage::start_depannage(&f.db, i).await.expect("dépannage");
    assert_eq!(done.outcome, DepannageOutcome::Started);
    assert_eq!(done.wo.status_code.as_deref(), Some("in_progress"));
    assert!(done.required_permit_types.is_empty() && done.permit_draft_ids.is_empty());
    assert_eq!(events_of(&f.db, PERMIT_NEEDED_CATEGORY, done.wo.id).await, 0);
}

#[tokio::test]
async fn the_breakdown_time_is_editable_until_close() {
    let f = fixture().await;
    let done = depannage::start_depannage(&f.db, input(&f, f.tech))
        .await
        .expect("dépannage");
    let earlier = now_minus(Duration::hours(3));
    let changed = depannage::set_breakdown_at(
        &f.db,
        WoBreakdownAtInput {
            wo_id: done.wo.id,
            expected_row_version: done.wo.row_version,
            breakdown_at: Some(earlier.clone()),
            actor_id: i64::from(f.tech),
        },
    )
    .await
    .expect("changed");
    assert_eq!(changed.wo.breakdown_at.as_deref(), Some(earlier.as_str()));
    assert_eq!(changed.wo.row_version, done.wo.row_version + 1);
    let audited = crate::wo::audit::wo_field_changes(&done.wo, &changed.wo);
    assert!(
        audited.iter().any(|c| c["field"] == "breakdown_at"),
        "the change is audited: {audited:?}"
    );

    // Never in the future.
    let err = depannage::set_breakdown_at(
        &f.db,
        WoBreakdownAtInput {
            wo_id: done.wo.id,
            expected_row_version: changed.wo.row_version,
            breakdown_at: Some(now_minus(Duration::hours(-2))),
            actor_id: i64::from(f.tech),
        },
    )
    .await
    .unwrap_err();
    refused_with(&err, CODE_BREAKDOWN_AT_FUTURE);

    // Only on a dépannage.
    let ordinary = crate::wo::queries::create_work_order(
        &f.db,
        crate::wo::domain::WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(f.eq_a),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_code: None,
            subtype_code: None,
            title: "Ordinary".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: i64::from(f.tech),
            requires_permit: None,
        },
    )
    .await
    .expect("ordinary work order");
    assert!(!ordinary.is_depannage && ordinary.breakdown_at.is_none() && ordinary.machine_stopped.is_none());
    let err = depannage::set_breakdown_at(
        &f.db,
        WoBreakdownAtInput {
            wo_id: ordinary.id,
            expected_row_version: ordinary.row_version,
            breakdown_at: Some(earlier),
            actor_id: i64::from(f.tech),
        },
    )
    .await
    .unwrap_err();
    refused_with(&err, CODE_NOT_A_DEPANNAGE);
}

#[tokio::test]
async fn a_received_depannage_must_carry_its_facts() {
    let f = fixture().await;
    let payload = |v: serde_json::Value| v.as_object().expect("object").clone();
    let now = crate::wo::time::now_utc_z();
    let db = &f.db;
    let check = |p| async move {
        depannage::inbound_depannage_check(db, "dep-received-wo", &p)
            .await
            .expect("check")
    };
    // An ordinary work order: not judged here.
    assert!(check(payload(serde_json::json!({ "code": "OT-R", "is_depannage": 0 })))
        .await
        .is_none());
    // A complete dépannage.
    assert!(check(payload(serde_json::json!({
        "code": "OT-R", "is_depannage": 1, "machine_stopped": 1,
        "breakdown_at": now_minus(Duration::hours(1)), "updated_at": now,
    })))
    .await
    .is_none());
    // Without "machine arrêtée", without a readable breakdown, or a breakdown after its row.
    for bad in [
        serde_json::json!({ "code": "OT-R", "is_depannage": 1, "machine_stopped": null,
                            "breakdown_at": now_minus(Duration::hours(1)), "updated_at": now }),
        serde_json::json!({ "code": "OT-R", "is_depannage": 1, "machine_stopped": 0,
                            "breakdown_at": "hier", "updated_at": now }),
        serde_json::json!({ "code": "OT-R", "is_depannage": 1, "machine_stopped": 0,
                            "breakdown_at": now_minus(Duration::hours(-3)), "updated_at": now }),
    ] {
        assert!(
            matches!(
                check(payload(bad.clone())).await,
                Some(crate::settings::site::InboundVerdict::Conflict { code, .. })
                    if code == depannage::CODE_SYNC_FACTS_INVALID
            ),
            "{bad}"
        );
    }
}

/// Through the inbound apply path (`sync::apply_hooks::before_upsert_checked`): a dépannage received
/// with its facts stripped, or turned back into an ordinary work order, is a conflict and is not
/// written; received unchanged with a newer edit, it is applied.
#[tokio::test]
async fn a_received_depannage_is_judged_on_the_inbound_path() {
    use crate::sync::materialize::ApplyOutcome;
    use sea_orm::TransactionTrait;

    let f = fixture().await;
    let done = depannage::start_depannage(&f.db, input(&f, f.tech))
        .await
        .expect("dépannage");
    let sync_id: String = {
        let row = sea_orm::ConnectionTrait::query_one(
            &f.db,
            sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                format!("SELECT entity_sync_id FROM work_orders WHERE id = {}", done.wo.id),
            ),
        )
        .await
        .expect("query")
        .expect("row");
        row.try_get("", "entity_sync_id").expect("sync id")
    };
    let local = crate::permit::sync_check::local_wire(&f.db, "work_orders", &sync_id)
        .await
        .expect("wire")
        .expect("local row");
    let apply = |payload: crate::permit::sync_check::Payload| {
        let db = &f.db;
        let sync_id = sync_id.clone();
        async move {
            let tx = db.begin().await.expect("begin");
            let outcome = crate::sync::materialize::apply_inbound_payload(
                &tx,
                "work_orders",
                &sync_id,
                "upsert",
                1,
                &serde_json::to_string(&payload).expect("json"),
            )
            .await
            .expect("apply");
            if matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged) {
                tx.commit().await.expect("commit");
            } else {
                tx.rollback().await.expect("rollback");
            }
            crate::sync::row_codec::clear_ref_cache();
            outcome
        }
    };
    let edited = |changes: &[(&str, serde_json::Value)]| {
        let mut p = local.clone();
        let rv = p.get("row_version").and_then(serde_json::Value::as_i64).unwrap_or(1);
        p.insert("row_version".into(), serde_json::json!(rv + 1));
        p.insert("updated_at".into(), serde_json::json!(crate::wo::time::now_utc_z()));
        for (k, v) in changes {
            p.insert((*k).to_string(), v.clone());
        }
        p
    };

    for bad in [
        edited(&[("machine_stopped", serde_json::Value::Null)]),
        edited(&[("breakdown_at", serde_json::Value::Null)]),
        edited(&[("is_depannage", serde_json::json!(0))]),
    ] {
        match apply(bad).await {
            ApplyOutcome::Conflict { code, .. } => assert_eq!(code, depannage::CODE_SYNC_FACTS_INVALID),
            other => panic!("expected a dépannage conflict, got {other:?}"),
        }
    }
    let kept = crate::wo::queries::get_work_order(&f.db, done.wo.id)
        .await
        .expect("get")
        .expect("wo");
    assert!(kept.is_depannage && kept.machine_stopped == Some(true) && kept.breakdown_at.is_some());

    // A valid received edit of the dépannage is applied.
    let outcome = apply(edited(&[("title", serde_json::json!("Convoyeur bloqué (vu)"))])).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
}

/// A dépannage received ready, requiring a permit and with none linked (waiting for it) is an
/// ordinary ready work order on the inbound path: applied, not a conflict.
#[tokio::test]
async fn a_received_depannage_waiting_for_its_permit_is_applied() {
    use crate::sync::materialize::ApplyOutcome;
    use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

    let f = fixture().await;
    let mut i = input(&f, f.tech);
    i.requires_permit = Some(true);
    let done = depannage::start_depannage(&f.db, i).await.expect("dépannage");
    assert_eq!(done.outcome, DepannageOutcome::WaitingForPermit);
    let sync_id: String =
        f.db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT entity_sync_id FROM work_orders WHERE id = {}", done.wo.id),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "entity_sync_id")
        .expect("sync id");
    let mut received = crate::permit::sync_check::local_wire(&f.db, "work_orders", &sync_id)
        .await
        .expect("wire")
        .expect("local row");
    let rv = received
        .get("row_version")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(1);
    received.insert("row_version".into(), serde_json::json!(rv + 1));
    received.insert("updated_at".into(), serde_json::json!(crate::wo::time::now_utc_z()));
    received.insert("title".into(), serde_json::json!("Convoyeur bloqué (permis attendu)"));
    assert!(depannage::inbound_depannage_check(&f.db, &sync_id, &received)
        .await
        .expect("check")
        .is_none());

    let tx = f.db.begin().await.expect("begin");
    let outcome = crate::sync::materialize::apply_inbound_payload(
        &tx,
        "work_orders",
        &sync_id,
        "upsert",
        1,
        &serde_json::to_string(&received).expect("json"),
    )
    .await
    .expect("apply");
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
    tx.commit().await.expect("commit");
    crate::sync::row_codec::clear_ref_cache();
    let kept = crate::wo::queries::get_work_order(&f.db, done.wo.id)
        .await
        .expect("get")
        .expect("wo");
    assert_eq!(kept.status_code.as_deref(), Some("ready"));
    assert!(kept.is_depannage && kept.requires_permit);
}

// ── Review fixes (L1-05 A) ────────────────────────────────────────────────────────────────────────

/// Everything a dépannage writes, counted.
async fn footprint(db: &DatabaseConnection) -> [i64; 7] {
    let mut out = [0; 7];
    for (slot, table) in out.iter_mut().zip([
        "work_orders",
        "wo_state_transition_log",
        "wo_action_events",
        "wo_change_events",
        "work_permits",
        "notification_events",
        "notifications",
    ]) {
        *slot = scalar(db, &format!("SELECT COUNT(*) FROM {table}")).await;
    }
    out
}

/// A qualification the assignee does not hold, asked by the dépannage's own work type: refused,
/// and nothing is left behind — no work order, no transition, no event, no permit draft, no notice.
#[tokio::test]
async fn a_qualification_refusal_leaves_nothing_behind() {
    let f = fixture().await;
    rule_on_depannage_type(&f.db, "DEP_HOT_WORK", "hot_work").await;
    let work_type =
        crate::wo::types::default_type_for_nature(&f.db, crate::wo::types::MaintenanceNature::CorrectiveImmediate)
            .await
            .expect("dépannage type");
    exec(
        &f.db,
        &format!(
            "INSERT INTO qualification_requirements (entity_sync_id, target_kind, work_order_type_id,
                skill_reference_value_id, coverage, created_at, updated_at)
             SELECT 'dep-qual-type-req', 'work_order_type', {}, rv.id, 'each', '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
               FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id
               JOIN reference_domains d ON d.id = rs.domain_id
              WHERE d.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.is_active = 1
              ORDER BY rv.id LIMIT 1",
            work_type.id
        ),
    )
    .await;
    assert_eq!(
        scalar(
            &f.db,
            "SELECT COUNT(*) FROM qualification_requirements WHERE entity_sync_id = 'dep-qual-type-req'"
        )
        .await,
        1,
        "a skill to require"
    );
    let issuing_lead = holder(
        &f.db,
        "dep_issuing_lead",
        f.t.site_a,
        &[
            OT_VIEW,
            OT_CREATE,
            OT_EDIT,
            OT_EMERGENCY_ASSIGN,
            crate::rbac::permissions::PTW_ISSUE,
        ],
    )
    .await;
    let before = footprint(&f.db).await;

    let mut i = input(&f, issuing_lead);
    i.assignee_id = Some(i64::from(f.colleague));
    i.permit_drafting = Some(map_of(&f.db, issuing_lead).await);
    let err = depannage::start_depannage(&f.db, i).await.unwrap_err();
    refused_with(&err, "QUALIFICATION_");
    assert_eq!(footprint(&f.db).await, before, "rolled back entirely");
}

/// Once the permit draft is being written, a failure is no longer "no draft": the dépannage fails
/// and nothing is kept (no work order with a draft half written).
#[tokio::test]
async fn a_failure_while_writing_a_permit_draft_fails_the_depannage() {
    let f = fixture().await;
    rule_on_depannage_type(&f.db, "DEP_HOT_WORK", "hot_work").await;
    let issuing_tech = holder(
        &f.db,
        "dep_issuing_tech",
        f.t.site_a,
        &[OT_VIEW, OT_CREATE, OT_EDIT, crate::rbac::permissions::PTW_ISSUE],
    )
    .await;
    // The written draft disappears before it is read back (`require_work_permit`: not found).
    exec(
        &f.db,
        "CREATE TRIGGER dep_test_lose_draft AFTER UPDATE OF code ON work_permits          BEGIN DELETE FROM work_permits WHERE id = NEW.id; END",
    )
    .await;
    let before = footprint(&f.db).await;
    let mut i = input(&f, issuing_tech);
    i.permit_drafting = Some(map_of(&f.db, issuing_tech).await);
    let err = depannage::start_depannage(&f.db, i).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound { .. }), "{err:?}");
    assert_eq!(footprint(&f.db).await, before, "rolled back entirely");
}

/// The assignment is in the work order's history as an assignment, with its origin; someone else
/// assigned is told at once through the assignment notice, once (the watcher's round adds none);
/// a user who assigns themself is not told.
#[tokio::test]
async fn the_assignment_is_in_the_history_and_another_assignee_is_told() {
    let f = fixture().await;
    let mut i = input(&f, f.lead);
    i.assignee_id = Some(i64::from(f.colleague));
    let wo_id = depannage::start_depannage(&f.db, i).await.expect("dépannage").wo.id;

    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM wo_action_events WHERE wo_id = {wo_id} AND action_code = 'emergency_reassigned'                    AND actor_id = {} AND json_extract(payload_json, '$.origin') = 'depannage'                    AND json_extract(payload_json, '$.primary_responsible_id') = {}",
                f.lead, f.colleague
            )
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {wo_id} AND action = 'assigned'                    AND actor_id = {} AND json_extract(details_json, '$.origin') = 'depannage'                    AND json_extract(details_json, '$.changes[0].field') = 'primary_responsible'",
                f.lead
            )
        )
        .await,
        1
    );
    assert!(
        told(&f.db, "wo_assigned", wo_id, f.colleague).await,
        "the assignee is told"
    );
    assert!(!told(&f.db, "wo_assigned", wo_id, f.lead).await, "not the lead");
    // Derived on each PC, never sent by synchronisation (as the watcher writes it).
    assert_eq!(scalar(&f.db, "SELECT COUNT(*) FROM sync_capture_suppression").await, 0);
    assert_eq!(
        scalar(
            &f.db,
            "SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id IN ( \
                SELECT entity_sync_id FROM notification_events WHERE category_code = 'wo_assigned' \
                UNION SELECT n.entity_sync_id FROM notifications n \
                  JOIN notification_events e ON e.id = n.notification_event_id WHERE e.category_code = 'wo_assigned')"
        )
        .await,
        0
    );
    crate::notifications::watchers::run_watcher(&f.db, "wo_assigned", chrono::Utc::now(), true)
        .await
        .expect("watcher round");
    assert_eq!(events_of(&f.db, "wo_assigned", wo_id).await, 1, "told once");

    let own = depannage::start_depannage(&f.db, input(&f, f.tech))
        .await
        .expect("own")
        .wo
        .id;
    assert_eq!(
        events_of(&f.db, "wo_assigned", own).await,
        0,
        "not told of their own dépannage"
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT COUNT(*) FROM wo_change_events WHERE wo_id = {own} AND action = 'assigned'")
        )
        .await,
        1,
        "in the history all the same"
    );
}
