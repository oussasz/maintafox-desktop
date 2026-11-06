//! Who may carry out a work order (L5-08, `wo::execution_actor`): on site A of the
//! `auth::scope_tests` tree, a work order assigned to a person and a team. Its assignee, a member
//! of its team, its planner and an `ot.approve` holder at its site carry it out; a colleague
//! holding `ot.edit` there sees it read-only except their own labour; the rule never lets anyone
//! act without the action's own right (`ot.close` to close).

use std::collections::HashMap;

use sea_orm::DatabaseConnection;

use crate::auth::scope::{self, AccessMap};
use crate::auth::scope_tests::{assign, code_of, exec, role, scalar, tree, user, Tree};
use crate::errors::AppError;
use crate::rbac::permissions::{OT_APPROVE, OT_CLOSE, OT_EDIT, OT_VIEW};
use crate::wo::access::WoChild;
use crate::wo::domain::WoCreateInput;
use crate::wo::execution_actor::{self as actor, codes, ActorRole};
use crate::wo::queries;

struct Fixture {
    db: DatabaseConnection,
    t: Tree,
    /// The work order of site A: assigned to `assignee` and team "Équipe Nord", planned by `planner`.
    wo: i64,
    assignee: i32,
    member: i32,
    planner: i32,
    /// `ot.approve` (with view and edit) on site A.
    approver: i32,
    /// `ot.approve` on site B only.
    approver_b: i32,
    /// `ot.edit` on site A, not on the work order.
    colleague: i32,
    /// `ot.edit` and `ot.close` on site A, not on the work order.
    colleague_closer: i32,
    /// Left the team (assignment ended).
    former_member: i32,
    /// The register cards of the colleague and of the assignee.
    colleague_card: i64,
    assignee_card: i64,
    team: i64,
}

async fn personnel_for(db: &DatabaseConnection, user_id: i32, code: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO personnel (employee_code, full_name, availability_status) \
             VALUES ('EXA-{code}', 'Card {code}', 'available')"
        ),
    )
    .await;
    let id = scalar(
        db,
        &format!("SELECT id FROM personnel WHERE employee_code = 'EXA-{code}'"),
    )
    .await;
    exec(
        db,
        &format!("UPDATE user_accounts SET personnel_id = {id} WHERE id = {user_id}"),
    )
    .await;
    id
}

async fn crew(db: &DatabaseConnection, personnel: i64, team: i64, valid_to: Option<&str>) {
    let valid_to = valid_to.map_or_else(|| "NULL".to_string(), |v| format!("'{v}'"));
    exec(
        db,
        &format!(
            "INSERT INTO personnel_team_assignments (personnel_id, team_id, role_code, allocation_percent, is_lead, \
                valid_from, valid_to) VALUES ({personnel}, {team}, 'member', 100.0, 0, '2020-01-01T00:00:00Z', {valid_to})"
        ),
    )
    .await;
}

async fn new_wo(db: &DatabaseConnection, t: &Tree, creator: i32, title: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                created_at, updated_at)
             VALUES ('exa-eq-{title}', 'EXA-{title}', 'Equipment {title}', 'active_in_service', {},
                     datetime('now'), datetime('now'))",
            t.site_a
        ),
    )
    .await;
    let equipment_id = scalar(
        db,
        &format!("SELECT id FROM equipment WHERE entity_sync_id = 'exa-eq-{title}'"),
    )
    .await;
    queries::create_work_order(
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
            description: Some(format!("{title} (execution actor test)")),
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

async fn fixture() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    let t = tree(&db).await;
    let assignee = user(&db, "exa_assignee").await;
    let member = user(&db, "exa_member").await;
    let planner = user(&db, "exa_planner").await;
    let approver = user(&db, "exa_approver").await;
    let approver_b = user(&db, "exa_approver_b").await;
    let colleague = user(&db, "exa_colleague").await;
    let colleague_closer = user(&db, "exa_colleague_closer").await;
    let former_member = user(&db, "exa_former").await;

    // The technicians: view and edit on site A, without the close right.
    let tech = role(&db, "EXA technician A", &[OT_VIEW, OT_EDIT]).await;
    for u in [assignee, member, planner, colleague, former_member] {
        assign(&db, u, tech, "org_node", Some(t.site_a)).await;
    }
    let closer = role(&db, "EXA closer A", &[OT_VIEW, OT_EDIT, OT_CLOSE]).await;
    assign(&db, colleague_closer, closer, "org_node", Some(t.site_a)).await;
    let approve = role(&db, "EXA approver", &[OT_VIEW, OT_EDIT, OT_APPROVE, OT_CLOSE]).await;
    assign(&db, approver, approve, "org_node", Some(t.site_a)).await;
    assign(&db, approver_b, approve, "org_node", Some(t.site_b)).await;
    // The site-B approver also edits on site A (as a technician): the approve right is not there.
    assign(&db, approver_b, tech, "org_node", Some(t.site_a)).await;

    exec(
        &db,
        &format!(
            "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at) \
             VALUES ('exa-team', 'EXA-NORD', 'Équipe Nord', 'maintenance', {}, 'active', datetime('now'), datetime('now'))",
            t.site_a
        ),
    )
    .await;
    let team = scalar(&db, "SELECT id FROM teams WHERE entity_sync_id = 'exa-team'").await;
    let member_card = personnel_for(&db, member, "MEMBER").await;
    crew(&db, member_card, team, None).await;
    let former_card = personnel_for(&db, former_member, "FORMER").await;
    crew(&db, former_card, team, Some("2021-01-01T00:00:00Z")).await;
    let colleague_card = personnel_for(&db, colleague, "COLLEAGUE").await;
    let assignee_card = personnel_for(&db, assignee, "ASSIGNEE").await;
    exec(
        &db,
        &format!("UPDATE user_accounts SET display_name = 'Karim Benali' WHERE id = {assignee}"),
    )
    .await;

    let wo = new_wo(&db, &t, assignee, "Leak").await;
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET primary_responsible_id = {assignee}, assigned_team_id = {team}, \
             planner_id = {planner} WHERE id = {wo}"
        ),
    )
    .await;
    Fixture {
        db,
        t,
        wo,
        assignee,
        member,
        planner,
        approver,
        approver_b,
        colleague,
        colleague_closer,
        former_member,
        colleague_card,
        assignee_card,
        team,
    }
}

/// The user's rights now (a fresh cache).
async fn map_of(db: &DatabaseConnection, user_id: i32) -> AccessMap {
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    scope::access_map_cached(db, &cache, user_id).await.expect("access map")
}

async fn execute(f: &Fixture, u: i32, permission: &str, wo: i64) -> Result<(), AppError> {
    actor::check_execution_on_wo(&f.db, &map_of(&f.db, u).await, i64::from(u), permission, wo).await
}

fn params_of(err: &AppError) -> HashMap<String, String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.params.clone()).unwrap_or_default(),
        _ => HashMap::new(),
    }
}

async fn task_of(db: &DatabaseConnection, wo: i64) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO work_order_tasks (work_order_id, task_description, sequence_order, is_mandatory, is_completed, \
             origin, entity_sync_id) VALUES ({wo}, 'Check the seal', 1, 1, 0, 'planned', 'exa-task-{wo}')"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM work_order_tasks WHERE entity_sync_id = 'exa-task-{wo}'"),
    )
    .await
}

async fn labour_entry(db: &DatabaseConnection, wo: i64, user_id: i32, personnel: Option<i64>, tag: &str) -> i64 {
    let personnel = personnel.map_or_else(|| "NULL".to_string(), |p| p.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO work_order_interveners (work_order_id, intervener_id, personnel_id, is_overtime, started_at, \
             entity_sync_id) VALUES ({wo}, {user_id}, {personnel}, 0, '2026-11-05T08:00:00Z', 'exa-labour-{tag}')"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM work_order_interveners WHERE entity_sync_id = 'exa-labour-{tag}'"),
    )
    .await
}

#[tokio::test]
async fn the_assignee_a_team_member_the_planner_and_a_site_approver_carry_it_out() {
    let f = fixture().await;
    for (u, expected) in [
        (f.assignee, ActorRole::Assignee),
        (f.member, ActorRole::TeamMember),
        (f.planner, ActorRole::Planner),
        (f.approver, ActorRole::Approver),
    ] {
        let standing = actor::standing(&f.db, &map_of(&f.db, u).await, i64::from(u), f.wo)
            .await
            .expect("standing");
        assert_eq!(standing.actor, Some(expected), "user {u}");
        assert!(standing.refusal.is_none());
        execute(&f, u, OT_EDIT, f.wo).await.expect("execution allowed");
    }
}

#[tokio::test]
async fn a_colleague_with_the_edit_right_is_refused_with_the_assignee_and_team_named() {
    let f = fixture().await;
    let err = execute(&f, f.colleague, OT_EDIT, f.wo).await.expect_err("refused");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
    let params = params_of(&err);
    assert_eq!(params.get("assignee").map(String::as_str), Some("Karim Benali"));
    assert_eq!(params.get("team").map(String::as_str), Some("Équipe Nord"));
    assert!(err
        .to_string()
        .contains("OT assigné à Karim Benali (équipe Équipe Nord)"));
    let standing = actor::standing(&f.db, &map_of(&f.db, f.colleague).await, i64::from(f.colleague), f.wo)
        .await
        .expect("standing");
    assert!(!standing.allowed());
    assert_eq!(standing.refusal.expect("reason").code, codes::ASSIGNED_PERSON_TEAM);
}

#[tokio::test]
async fn a_former_team_member_and_an_approver_of_another_site_are_refused() {
    let f = fixture().await;
    for u in [f.former_member, f.approver_b] {
        let err = execute(&f, u, OT_EDIT, f.wo).await.expect_err("refused");
        assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM, "user {u}");
    }
}

#[tokio::test]
async fn the_rule_never_stands_in_for_the_close_right() {
    let f = fixture().await;
    // The assignee without ot.close: refused for the right, as before.
    let err = execute(&f, f.assignee, OT_CLOSE, f.wo)
        .await
        .expect_err("no close right");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
    // A colleague with ot.close: refused by the rule.
    let err = execute(&f, f.colleague_closer, OT_CLOSE, f.wo)
        .await
        .expect_err("not an actor");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
    // The site approver with ot.close closes.
    execute(&f, f.approver, OT_CLOSE, f.wo).await.expect("approver closes");
    // Someone named on the work order without the edit right at all: refused for the right.
    let outsider = user(&f.db, "exa_outsider").await;
    exec(
        &f.db,
        &format!("UPDATE work_orders SET planner_id = {outsider} WHERE id = {}", f.wo),
    )
    .await;
    let err = execute(&f, outsider, OT_EDIT, f.wo).await.expect_err("no right");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
}

#[tokio::test]
async fn task_results_follow_the_rule() {
    let f = fixture().await;
    let task = task_of(&f.db, f.wo).await;
    let on_task = |u: i32| {
        let db = &f.db;
        async move {
            actor::check_execution_on_child(db, &map_of(db, u).await, i64::from(u), OT_EDIT, WoChild::Task, task).await
        }
    };
    on_task(f.member).await.expect("team member records the result");
    let err = on_task(f.colleague).await.expect_err("colleague refused");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
    let err = actor::check_execution_on_child(
        &f.db,
        &map_of(&f.db, f.colleague).await,
        i64::from(f.colleague),
        OT_EDIT,
        WoChild::Task,
        999_999,
    )
    .await
    .expect_err("no such task");
    assert!(matches!(err, AppError::NotFound { .. }));
}

#[tokio::test]
async fn a_colleague_records_their_own_labour_only() {
    let f = fixture().await;
    let map = map_of(&f.db, f.colleague).await;
    let me = i64::from(f.colleague);
    // Own login, or own register card.
    actor::check_labor_for(&f.db, &map, me, f.wo, Some(me), None)
        .await
        .expect("own login");
    actor::check_labor_for(&f.db, &map, me, f.wo, None, Some(f.colleague_card))
        .await
        .expect("own card");
    // Somebody else's.
    let err = actor::check_labor_for(&f.db, &map, me, f.wo, Some(i64::from(f.assignee)), None)
        .await
        .expect_err("another login");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
    let err = actor::check_labor_for(&f.db, &map, me, f.wo, None, Some(f.assignee_card))
        .await
        .expect_err("another card");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
    // Closing an entry: their own, never the assignee's.
    let own = labour_entry(&f.db, f.wo, f.colleague, Some(f.colleague_card), "own").await;
    let theirs = labour_entry(&f.db, f.wo, f.assignee, Some(f.assignee_card), "theirs").await;
    actor::check_labor_entry(&f.db, &map, me, own).await.expect("own entry");
    let err = actor::check_labor_entry(&f.db, &map, me, theirs)
        .await
        .expect_err("assignee's entry");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
    // An execution actor keeps the command's own rules for booking other people.
    let assignee_map = map_of(&f.db, f.assignee).await;
    actor::check_labor_for(
        &f.db,
        &assignee_map,
        i64::from(f.assignee),
        f.wo,
        None,
        Some(f.colleague_card),
    )
    .await
    .expect("assignee books a colleague");
}

#[tokio::test]
async fn the_reason_names_who_it_is_assigned_to() {
    let f = fixture().await;
    let colleague = |wo: i64| execute(&f, f.colleague, OT_EDIT, wo);

    // A person only.
    exec(
        &f.db,
        &format!("UPDATE work_orders SET assigned_team_id = NULL WHERE id = {}", f.wo),
    )
    .await;
    let err = colleague(f.wo).await.expect_err("person only");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON);
    assert_eq!(
        params_of(&err).get("assignee").map(String::as_str),
        Some("Karim Benali")
    );

    // A team only: its members carry it out.
    exec(
        &f.db,
        &format!(
            "UPDATE work_orders SET primary_responsible_id = NULL, assigned_team_id = {} WHERE id = {}",
            f.team, f.wo
        ),
    )
    .await;
    let err = colleague(f.wo).await.expect_err("team only");
    assert_eq!(code_of(&err), codes::ASSIGNED_TEAM);
    assert_eq!(params_of(&err).get("team").map(String::as_str), Some("Équipe Nord"));
    execute(&f, f.member, OT_EDIT, f.wo).await.expect("member");
    let err = execute(&f, f.assignee, OT_EDIT, f.wo)
        .await
        .expect_err("no longer assigned");
    assert_eq!(code_of(&err), codes::ASSIGNED_TEAM);
}

#[tokio::test]
async fn a_work_order_assigned_to_nobody_is_carried_out_by_its_planner_or_an_approver() {
    let f = fixture().await;
    let wo = new_wo(&f.db, &f.t, f.colleague, "Unassigned").await;
    // Its creator, with the edit right, is not its actor.
    let err = execute(&f, f.colleague, OT_EDIT, wo)
        .await
        .expect_err("creator refused");
    assert_eq!(code_of(&err), codes::UNASSIGNED);
    assert!(params_of(&err).is_empty());
    execute(&f, f.approver, OT_EDIT, wo).await.expect("approver");
    exec(
        &f.db,
        &format!("UPDATE work_orders SET planner_id = {} WHERE id = {wo}", f.planner),
    )
    .await;
    execute(&f, f.planner, OT_EDIT, wo).await.expect("planner");
    let err = execute(&f, f.member, OT_EDIT, wo)
        .await
        .expect_err("member of another team");
    assert_eq!(code_of(&err), codes::UNASSIGNED);
}

#[tokio::test]
async fn rights_are_judged_as_held_now() {
    let f = fixture().await;
    execute(&f, f.approver, OT_EDIT, f.wo).await.expect("approver now");
    // The approve right withdrawn: the approver is now a colleague.
    exec(
        &f.db,
        &format!(
            "UPDATE user_scope_assignments SET deleted_at = datetime('now') WHERE user_id = {}",
            f.approver
        ),
    )
    .await;
    let tech = scalar(&f.db, "SELECT id FROM roles WHERE name = 'EXA technician A'").await;
    assign(&f.db, f.approver, tech, "org_node", Some(f.t.site_a)).await;
    let err = execute(&f, f.approver, OT_EDIT, f.wo)
        .await
        .expect_err("approve right gone");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
}

#[tokio::test]
async fn downtime_entries_follow_the_rule() {
    let f = fixture().await;
    exec(
        &f.db,
        &format!(
            "INSERT INTO work_order_downtime_segments (work_order_id, started_at, ended_at, downtime_type) \
             VALUES ({}, '2026-11-05T08:00:00Z', NULL, 'full')",
            f.wo
        ),
    )
    .await;
    let segment = scalar(
        &f.db,
        &format!(
            "SELECT MAX(id) FROM work_order_downtime_segments WHERE work_order_id = {}",
            f.wo
        ),
    )
    .await;
    let on_segment = |u: i32| {
        let db = &f.db;
        async move {
            actor::check_execution_on_child(
                db,
                &map_of(db, u).await,
                i64::from(u),
                OT_EDIT,
                WoChild::Downtime,
                segment,
            )
            .await
        }
    };
    on_segment(f.assignee).await.expect("assignee closes the downtime");
    let err = on_segment(f.colleague).await.expect_err("colleague refused");
    assert_eq!(code_of(&err), codes::ASSIGNED_PERSON_TEAM);
}

#[tokio::test]
async fn the_creator_cancels_their_own_draft_but_not_once_it_is_released() {
    let f = fixture().await;
    let cancel = |u: i32, wo: i64| {
        let db = &f.db;
        async move { actor::check_cancel(db, &map_of(db, u).await, i64::from(u), OT_EDIT, wo).await }
    };
    // A draft assigned to nobody, created by the colleague.
    let wo = new_wo(&f.db, &f.t, f.colleague, "Own draft").await;
    let status: String = {
        let row = sea_orm::ConnectionTrait::query_one(
            &f.db,
            sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                format!("SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = {wo}"),
            ),
        )
        .await
        .expect("query")
        .expect("row");
        row.try_get("", "code").expect("code")
    };
    assert_eq!(status, "draft");
    cancel(f.colleague, wo).await.expect("creator cancels their draft");
    let standing = actor::standing(&f.db, &map_of(&f.db, f.colleague).await, i64::from(f.colleague), wo)
        .await
        .expect("standing");
    assert!(standing.own_draft);
    assert!(!standing.allowed(), "the other execution actions stay closed");
    // Another ot.edit holder cannot cancel it.
    let err = cancel(f.member, wo).await.expect_err("not their draft");
    assert_eq!(code_of(&err), codes::UNASSIGNED);
    // Released (submitted to planning): planner or approver only.
    exec(
        &f.db,
        &format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'planning') \
             WHERE id = {wo}"
        ),
    )
    .await;
    let err = cancel(f.colleague, wo).await.expect_err("released");
    assert_eq!(code_of(&err), codes::UNASSIGNED);
    cancel(f.approver, wo).await.expect("approver cancels");
    // The creator without the edit right: refused for the right, whatever the draft.
    let err = actor::check_cancel(
        &f.db,
        &map_of(&f.db, f.colleague).await,
        i64::from(f.colleague),
        OT_CLOSE,
        wo,
    )
    .await
    .expect_err("no such right");
    assert_eq!(code_of(&err), "PERMISSION_DENIED");
}
