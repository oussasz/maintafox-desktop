//! Inbox, subscriptions, watchers, resolution and escalation, on a seeded database.

use chrono::{Duration, Utc};
use sea_orm::DatabaseConnection;

use crate::notifications::catalog::MODULES;
use crate::notifications::inbox::{self, InboxFilter};
use crate::notifications::{conditions, config, delivery, emitter, scheduler, watchers};
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text};

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

async fn db() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    db
}

async fn person(db: &DatabaseConnection, id: i64, supervisor: Option<i64>) {
    exec(
        db,
        "INSERT INTO personnel (id, employee_code, full_name, availability_status, supervisor_id) \
         VALUES (?, ?, ?, 'available', ?)",
        vec![
            id.into(),
            format!("P-{id}").into(),
            format!("Person {id}").into(),
            supervisor.into(),
        ],
    )
    .await;
}

async fn user(db: &DatabaseConnection, id: i64, personnel: Option<i64>) {
    exec(
        db,
        &format!(
            "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active, \
             is_admin, force_password_change, personnel_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'local', 1, 0, 0, ?, {NOW}, {NOW})"
        ),
        vec![
            id.into(),
            format!("nt-user-{id}").into(),
            format!("user{id}").into(),
            format!("User {id}").into(),
            personnel.into(),
        ],
    )
    .await;
}

/// A role holding `permissions`, granted plant-wide to `user_id`.
async fn grant(db: &DatabaseConnection, user_id: i64, permissions: &[&str]) {
    let role = format!("nt-role-{user_id}");
    exec(
        db,
        &format!(
            "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at) \
             VALUES (?, ?, '', 0, 'custom', 'active', {NOW}, {NOW})"
        ),
        vec![role.clone().into(), role.clone().into()],
    )
    .await;
    let role_id = scalar_i64(db, "SELECT id FROM roles WHERE name = ?", vec![role.into()]).await;
    for p in permissions {
        exec(
            db,
            &format!("INSERT INTO role_permissions (role_id, permission_id, granted_at) SELECT ?, id, {NOW} FROM permissions WHERE name = ?"),
            vec![role_id.into(), (*p).into()],
        )
        .await;
    }
    exec(
        db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES (?, ?, ?, 'tenant', {NOW}, {NOW})"
        ),
        vec![format!("nt-usa-{user_id}").into(), user_id.into(), role_id.into()],
    )
    .await;
}

async fn status_id(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM work_order_statuses WHERE code = ?",
        vec![code.into()],
    )
    .await
}

/// An open work order (`ready`) with its responsible, planner and planned end.
async fn work_order(
    db: &DatabaseConnection,
    id: i64,
    assignee: Option<i64>,
    planner: Option<i64>,
    planned_end: Option<&str>,
) {
    let ready = status_id(db, "ready").await;
    exec(
        db,
        &format!(
            "INSERT INTO work_orders (id, code, type_id, status_id, primary_responsible_id, planner_id, title, \
             planned_end, entity_sync_id, created_at, updated_at) VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, {NOW}, {NOW})"
        ),
        vec![
            id.into(),
            format!("OT-{id}").into(),
            ready.into(),
            assignee.into(),
            planner.into(),
            format!("Pump {id}").into(),
            planned_end.map(str::to_string).into(),
            format!("nt-wo-{id}").into(),
        ],
    )
    .await;
}

fn all_modules() -> inbox::Visibility {
    inbox::Visibility::all()
}

async fn tick(db: &DatabaseConnection) {
    scheduler::run_scheduler_tick_in(db, Utc::now(), false)
        .await
        .expect("tick");
}

async fn notifications_of(db: &DatabaseConnection, user_id: i64, category: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE n.recipient_user_id = ? AND e.category_code = ?",
        vec![user_id.into(), category.into()],
    )
    .await
}

fn past(minutes: i64) -> String {
    (Utc::now() - Duration::minutes(minutes))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[tokio::test]
async fn migration_ships_categories_with_modules_and_escalation_reaching_real_targets() {
    let db = db().await;
    // Every offered category belongs to a module the inbox knows (else nobody would see it).
    let unknown = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM notification_categories WHERE is_offered = 1 AND (module_code IS NULL OR module_code NOT IN ({}))",
            MODULES.iter().map(|(m, _)| format!("'{m}'")).collect::<Vec<_>>().join(",")
        ),
        vec![],
    )
    .await;
    assert_eq!(unknown, 0);
    // Every offered category has a rule with audiences; planning messages are routed now.
    let without_rule = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM notification_categories c WHERE c.is_offered = 1 AND c.code <> 'daily_digest' AND NOT EXISTS \
         (SELECT 1 FROM notification_rules r WHERE r.category_code = c.code AND r.audiences_json IS NOT NULL)",
        vec![],
    )
    .await;
    assert_eq!(without_rule, 0);
    assert_eq!(
        scalar_opt_text(
            &db,
            "SELECT audiences_json FROM notification_rules WHERE category_code = 'planning.schedule'",
            vec![]
        )
        .await
        .as_deref(),
        Some(r#"["team"]"#)
    );
    // The shipped steps target people who exist (they named roles no installation has).
    let targets = scalar_opt_text(
        &db,
        "SELECT group_concat(level_no || ':' || wait_minutes || ':' || route_to_type || ':' || COALESCE(route_to_reference, ''), ' ') \
         FROM (SELECT * FROM notification_escalation_steps ORDER BY level_no)",
        vec![],
    )
    .await;
    assert_eq!(
        targets.as_deref(),
        Some("1:30:supervisor: 2:60:entity_manager: 3:120:permission:ot.approve")
    );
    // Categories without a generator are not offered.
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM notification_categories WHERE is_offered = 0",
            vec![]
        )
        .await,
        2
    );
}

#[tokio::test]
async fn inbox_shows_only_modules_the_reader_may_view_and_the_bell_agrees() {
    let db = db().await;
    user(&db, 10, None).await;
    grant(&db, 10, &[crate::rbac::permissions::DI_VIEW]).await;
    work_order(&db, 1, Some(10), None, None).await;
    exec(
        &db,
        "UPDATE work_orders SET updated_at = ? WHERE id = 1",
        vec![past(60).into()],
    )
    .await;
    // The assignment that existed when watching started is listed read; the new one is notified.
    tick(&db).await;
    work_order(&db, 2, Some(10), None, None).await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 10, "wo_assigned").await, 2);

    let visible = inbox::visible_to(&db, 10).await.expect("visibility");
    assert!(visible.sees_module("di") && !visible.sees_module("ot"));
    // No ot.view: the work-order notification is neither listed nor counted.
    let listed = inbox::list(&db, 10, &visible, &InboxFilter::default())
        .await
        .expect("list");
    assert!(listed.is_empty());
    assert_eq!(inbox::unread_count(&db, 10, &visible).await.expect("count"), 0);
    let nid = scalar_i64(&db, "SELECT id FROM notifications WHERE recipient_user_id = 10", vec![]).await;
    assert!(inbox::ensure_visible(&db, 10, &visible, nid).await.is_err());

    // With the module visible, list, bell and counts agree.
    let modules = all_modules();
    let unread = InboxFilter {
        view: Some("unread".into()),
        ..InboxFilter::default()
    };
    let items = inbox::list(&db, 10, &modules, &unread).await.expect("list");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.module_code, "ot");
    assert_eq!(item.reason_code.as_deref(), Some("assignee"));
    assert_eq!(item.message_key.as_deref(), Some("wo_assigned"));
    assert_eq!(
        item.message_params.as_ref().and_then(|p| p["code"].as_str()),
        Some("OT-2")
    );
    assert_eq!(item.action_url.as_deref(), Some("/work-orders?openWo=2"));
    let counts = inbox::counts(&db, 10, &modules).await.expect("counts");
    assert_eq!(counts.unread, 1);
    assert_eq!(inbox::unread_count(&db, 10, &modules).await.expect("bell"), 1);

    assert_eq!(
        inbox::mark_all_read(&db, 10, &modules, &InboxFilter::default())
            .await
            .expect("all"),
        1
    );
    assert_eq!(inbox::unread_count(&db, 10, &modules).await.expect("bell"), 0);
    assert_eq!(
        inbox::list(&db, 10, &modules, &InboxFilter::default())
            .await
            .expect("inbox")
            .len(),
        2
    );
}

#[tokio::test]
async fn overdue_work_order_notifies_assignee_and_planner_once_and_resolves_when_closed() {
    let db = db().await;
    user(&db, 20, None).await;
    user(&db, 21, None).await;
    tick(&db).await;
    work_order(&db, 5, Some(20), Some(21), Some(&past(90))).await;
    tick(&db).await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 20, "wo_overdue").await, 1, "deduplicated");
    assert_eq!(notifications_of(&db, 21, "wo_overdue").await, 1);
    let reason = scalar_opt_text(
        &db,
        "SELECT n.reason_code FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'wo_overdue' AND n.recipient_user_id = 21",
        vec![],
    )
    .await;
    assert_eq!(reason.as_deref(), Some("planner"));
    let modules = all_modules();
    let before = inbox::unread_count(&db, 20, &modules).await.expect("bell");

    let closed = status_id(&db, "closed").await;
    exec(
        &db,
        "UPDATE work_orders SET status_id = ? WHERE id = 5",
        vec![closed.into()],
    )
    .await;
    tick(&db).await;
    let open = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code IN ('wo_overdue', 'wo_assigned') AND n.resolved_at IS NULL",
        vec![],
    )
    .await;
    assert_eq!(open, 0, "closing the order resolves its notifications");
    assert!(inbox::unread_count(&db, 20, &modules).await.expect("bell") < before);

    // Reopened and late again with a new planned end: a new occurrence is raised.
    let ready = status_id(&db, "ready").await;
    exec(
        &db,
        "UPDATE work_orders SET status_id = ?, planned_end = ? WHERE id = 5",
        vec![ready.into(), past(10).into()],
    )
    .await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 20, "wo_overdue").await, 2);
}

#[tokio::test]
async fn reassignment_resolves_the_old_assignment_and_notifies_the_new_assignee() {
    let db = db().await;
    user(&db, 30, None).await;
    user(&db, 31, None).await;
    tick(&db).await;
    work_order(&db, 7, Some(30), None, None).await;
    tick(&db).await;
    exec(
        &db,
        "UPDATE work_orders SET primary_responsible_id = 31 WHERE id = 7",
        vec![],
    )
    .await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 31, "wo_assigned").await, 1);
    let old_resolved = scalar_opt_text(
        &db,
        "SELECT n.resolved_at FROM notifications n WHERE n.recipient_user_id = 30",
        vec![],
    )
    .await;
    assert!(old_resolved.is_some());
}

#[tokio::test]
async fn a_category_turned_off_by_the_user_is_not_delivered() {
    let db = db().await;
    user(&db, 40, None).await;
    exec(
        &db,
        "INSERT INTO notification_preferences (user_id, category_code, in_app_enabled) VALUES (40, 'wo_assigned', 0)",
        vec![],
    )
    .await;
    tick(&db).await;
    work_order(&db, 8, Some(40), None, None).await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 40, "wo_assigned").await, 0);
}

#[tokio::test]
async fn stock_at_reorder_point_goes_to_the_inventory_managers_and_clears_when_restocked() {
    let db = db().await;
    user(&db, 50, None).await;
    user(&db, 51, None).await;
    grant(&db, 50, &[crate::rbac::permissions::INV_MANAGE]).await;
    tick(&db).await;
    exec(
        &db,
        &format!(
            "INSERT INTO articles (id, article_code, article_name, unit_value_id, reorder_point, is_active, entity_sync_id, \
             created_at, updated_at) VALUES (900, 'ART-900', 'Bearing', 1, 5, 1, 'nt-art-900', {NOW}, {NOW})"
        ),
        vec![],
    )
    .await;
    exec(
        &db,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, available_qty) VALUES (900, 1, 1, 3, 3)",
        vec![],
    )
    .await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 50, "stock_critical").await, 1);
    assert_eq!(notifications_of(&db, 51, "stock_critical").await, 0);
    let why = scalar_opt_text(
        &db,
        "SELECT reason_code || ':' || reason_ref FROM notifications WHERE recipient_user_id = 50",
        vec![],
    )
    .await;
    assert_eq!(why.as_deref(), Some("permission:inv.manage"));

    exec(
        &db,
        "UPDATE stock_balances SET available_qty = 20 WHERE article_id = 900",
        vec![],
    )
    .await;
    tick(&db).await;
    assert!(scalar_opt_text(
        &db,
        "SELECT resolved_at FROM notifications WHERE recipient_user_id = 50",
        vec![]
    )
    .await
    .is_some());
}

#[tokio::test]
async fn unacknowledged_critical_notification_escalates_to_the_supervisor_then_stops_on_acknowledgement() {
    let db = db().await;
    person(&db, 1, None).await;
    person(&db, 2, Some(1)).await;
    user(&db, 60, Some(1)).await; // supervisor
    user(&db, 61, Some(2)).await; // technician
    exec(
        &db,
        "UPDATE notification_rules SET requires_ack = 1, escalation_policy_id = \
         (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation') \
         WHERE category_code = 'wo_overdue'",
        vec![],
    )
    .await;
    tick(&db).await;
    work_order(&db, 9, Some(61), None, Some(&past(30))).await;
    tick(&db).await;
    assert_eq!(notifications_of(&db, 61, "wo_overdue").await, 1);

    // Before the first delay: nothing.
    let now = Utc::now();
    assert_eq!(scheduler::run_escalation_sweep(&db, now).await.expect("sweep"), 0);
    // 31 minutes later: the technician's supervisor is notified, with the reason.
    let later = now + Duration::minutes(31);
    assert_eq!(scheduler::run_escalation_sweep(&db, later).await.expect("sweep"), 1);
    assert_eq!(
        scheduler::run_escalation_sweep(&db, later).await.expect("sweep"),
        0,
        "once per level"
    );
    let why = scalar_opt_text(
        &db,
        "SELECT reason_code || ' ' || reason_ref FROM notifications WHERE recipient_user_id = 60",
        vec![],
    )
    .await;
    assert_eq!(why.as_deref(), Some("escalation 1:30"));
    let logged = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM activity_events WHERE event_code = 'notification.escalated'",
        vec![],
    )
    .await;
    assert_eq!(logged, 1);

    // Level 2 (site responsibles) finds nobody: logged, and the event still moves on.
    assert_eq!(
        scheduler::run_escalation_sweep(&db, now + Duration::minutes(61))
            .await
            .expect("sweep"),
        1
    );
    let level = scalar_i64(
        &db,
        "SELECT MAX(escalation_level) FROM notifications WHERE recipient_user_id = 61",
        vec![],
    )
    .await;
    assert_eq!(level, 2);

    // The supervisor acknowledges: no level 3.
    let sup = scalar_i64(&db, "SELECT id FROM notifications WHERE recipient_user_id = 60", vec![]).await;
    delivery::acknowledge(&db, sup, 60, Some("On it".into()))
        .await
        .expect("ack");
    assert_eq!(
        scheduler::run_escalation_sweep(&db, now + Duration::minutes(121))
            .await
            .expect("sweep"),
        0
    );
    // Nobody has it left to acknowledge.
    let modules = all_modules();
    let to_ack = InboxFilter {
        view: Some("to_acknowledge".into()),
        ..InboxFilter::default()
    };
    assert!(inbox::list(&db, 61, &modules, &to_ack).await.expect("list").is_empty());
    let items = inbox::list(&db, 61, &modules, &InboxFilter::default())
        .await
        .expect("list");
    assert_eq!(items[0].event_acknowledged_by.as_deref(), Some("User 60"));
}

#[tokio::test]
async fn snooze_archive_and_unread_follow_the_inbox_views() {
    let db = db().await;
    user(&db, 70, None).await;
    tick(&db).await;
    work_order(&db, 11, Some(70), None, None).await;
    tick(&db).await;
    let modules = all_modules();
    let id = scalar_i64(&db, "SELECT id FROM notifications WHERE recipient_user_id = 70", vec![]).await;
    let view = |v: &str| InboxFilter {
        view: Some(v.into()),
        ..InboxFilter::default()
    };

    assert!(delivery::snooze_until(&db, id, 70, Utc::now() + Duration::days(8))
        .await
        .is_err());
    assert!(delivery::snooze_until(&db, id, 70, Utc::now() - Duration::minutes(1))
        .await
        .is_err());
    delivery::snooze_until(&db, id, 70, Utc::now() + Duration::hours(1))
        .await
        .expect("snooze");
    assert!(inbox::list(&db, 70, &modules, &view("inbox"))
        .await
        .expect("l")
        .is_empty());
    assert_eq!(
        inbox::list(&db, 70, &modules, &view("snoozed")).await.expect("l").len(),
        1
    );
    assert_eq!(inbox::unread_count(&db, 70, &modules).await.expect("c"), 0);
    // Back when its time came, unread.
    exec(
        &db,
        "UPDATE notifications SET snoozed_until = ? WHERE id = ?",
        vec![past(1).into(), id.into()],
    )
    .await;
    tick(&db).await;
    assert_eq!(inbox::unread_count(&db, 70, &modules).await.expect("c"), 1);

    delivery::set_archived(&db, id, 70, true).await.expect("archive");
    assert!(inbox::list(&db, 70, &modules, &view("inbox"))
        .await
        .expect("l")
        .is_empty());
    assert_eq!(
        inbox::list(&db, 70, &modules, &view("archived"))
            .await
            .expect("l")
            .len(),
        1
    );
    delivery::set_archived(&db, id, 70, false).await.expect("restore");
    delivery::mark_as_unread(&db, id, 70).await.expect("unread");
    assert_eq!(inbox::unread_count(&db, 70, &modules).await.expect("c"), 1);
    // Someone else's notification cannot be touched.
    user(&db, 71, None).await;
    assert!(delivery::set_archived(&db, id, 71, true).await.is_err());
}

#[tokio::test]
async fn request_to_review_resolves_when_it_leaves_review_and_a_new_review_notifies_again() {
    let db = db().await;
    user(&db, 80, None).await;
    exec(
        &db,
        &format!(
            "INSERT INTO intervention_requests (id, code, equipment_id, org_node_id, status, title, description, origin_type, \
             impact_level, production_impact, safety_flag, environmental_flag, quality_flag, reported_urgency, observed_at, \
             submitted_at, reviewer_id, submitter_id, entity_sync_id, created_at, updated_at, row_version) \
             VALUES (300, 'DI-300', 1, 1, 'in_review', 'Leak', 'Oil leak', 'operator', 'minor', 0, 0, 0, 0, 'high', {NOW}, \
             {NOW}, 80, 80, 'nt-di-300', {NOW}, {NOW}, 1)"
        ),
        vec![],
    )
    .await;
    let input = || emitter::NotificationEventInput {
        source_module: "di".into(),
        source_record_id: Some("300".into()),
        event_code: "di_entered_in_review".into(),
        category_code: "di_pending_review".into(),
        severity: "info".into(),
        dedupe_key: Some("di-di_entered_in_review-300".into()),
        payload_json: None,
        title: "DI en revue — DI-300".into(),
        body: None,
        action_url: Some("/requests?openDi=300".into()),
    };
    emitter::emit_event(&db, input()).await.expect("emit");
    assert_eq!(notifications_of(&db, 80, "di_pending_review").await, 1);
    exec(
        &db,
        "UPDATE intervention_requests SET status = 'returned_for_clarification' WHERE id = 300",
        vec![],
    )
    .await;
    conditions::resolve_cleared(&db).await.expect("resolve");
    assert!(scalar_opt_text(
        &db,
        "SELECT resolved_at FROM notifications WHERE recipient_user_id = 80",
        vec![]
    )
    .await
    .is_some());
    // Back in review: the same de-duplication key raises a new notification.
    exec(
        &db,
        "UPDATE intervention_requests SET status = 'in_review' WHERE id = 300",
        vec![],
    )
    .await;
    emitter::emit_event(&db, input()).await.expect("emit again");
    assert_eq!(notifications_of(&db, 80, "di_pending_review").await, 2);
}

#[tokio::test]
async fn plant_settings_validate_audiences_delays_and_targets() {
    let db = db().await;
    let cfg = config::load(&db).await.expect("config");
    let overdue = cfg
        .rules
        .iter()
        .find(|r| r.category_code == "wo_overdue")
        .expect("rule");
    assert_eq!(overdue.audiences, vec!["assignee".to_string(), "planner".to_string()]);
    assert!(
        cfg.rules.iter().all(|r| r.category_code != "support_response"),
        "not offered"
    );
    assert_eq!(cfg.retention.expiry_days, scheduler::DEFAULT_EXPIRY_DAYS);
    assert!(cfg.retention.is_default);

    let update = |audiences: Vec<&str>| config::UpdateRuleInput {
        rule_id: overdue.rule_id,
        is_active: None,
        audiences: Some(audiences.into_iter().map(str::to_string).collect()),
        requires_ack: None,
        escalation_policy_id: None,
        lead_time_minutes: None,
    };
    assert!(config::update_rule(&db, &update(vec!["boss"])).await.is_err());
    config::update_rule(&db, &update(vec!["team_lead", "permission:ot.approve"]))
        .await
        .expect("update");
    let cfg = config::load(&db).await.expect("config");
    let overdue = cfg
        .rules
        .iter()
        .find(|r| r.category_code == "wo_overdue")
        .expect("rule");
    assert_eq!(
        overdue.audiences,
        vec!["team_lead".to_string(), "permission:ot.approve".to_string()]
    );
    // The budget alert names its recipient in its own configuration.
    let budget = cfg
        .rules
        .iter()
        .find(|r| r.category_code == "budget_control_alert")
        .expect("budget");
    assert!(budget.fixed_routing);
    assert!(config::update_rule(
        &db,
        &config::UpdateRuleInput {
            rule_id: budget.rule_id,
            is_active: None,
            audiences: Some(vec!["assignee".into()]),
            requires_ack: None,
            escalation_policy_id: None,
            lead_time_minutes: None,
        }
    )
    .await
    .is_err());

    let policy = &cfg.policies[0];
    let level2 = policy.steps.iter().find(|s| s.level_no == 2).expect("level 2");
    let step = |wait: i64, target: &str, reference: Option<&str>| config::EscalationStepInput {
        wait_minutes: wait,
        target: target.into(),
        reference: reference.map(str::to_string),
    };
    // Delays stay in level order; targets and references are checked.
    assert!(config::update_step(&db, level2.id, &step(20, "planner", None))
        .await
        .is_err());
    assert!(config::update_step(&db, level2.id, &step(90, "boss", None))
        .await
        .is_err());
    assert!(
        config::update_step(&db, level2.id, &step(90, "permission", Some("no.such")))
            .await
            .is_err()
    );
    config::update_step(&db, level2.id, &step(90, "planner", None))
        .await
        .expect("update step");
    config::add_step(
        &db,
        policy.id,
        &step(240, "permission", Some(crate::rbac::permissions::ADM_SETTINGS)),
    )
    .await
    .expect("add");
    let level1 = policy.steps.iter().find(|s| s.level_no == 1).expect("level 1");
    assert!(
        config::remove_step(&db, level1.id).await.is_err(),
        "only the last level"
    );
    let cfg = config::load(&db).await.expect("config");
    let last = cfg.policies[0].steps.last().expect("last");
    assert_eq!((last.level_no, last.wait_minutes), (4, 240));
    config::remove_step(&db, last.id).await.expect("remove last");
    assert!(config::validate_expiry_days(3).is_err());
}

#[tokio::test]
async fn ptw_expiring_uses_the_rule_lead_time() {
    let db = db().await;
    tick(&db).await;
    let soon = (Utc::now() + Duration::minutes(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    exec(
        &db,
        "INSERT INTO work_permits (id, entity_sync_id, code, permit_type_id, asset_id, entity_id, status, expires_at, row_version) \
         VALUES (500, 'nt-wp-500', 'PTW-500', 1, 1, 1, 'active', ?, 1)",
        vec![soon.into()],
    )
    .await;
    let r = watchers::run_watcher(&db, "ptw_expiring", Utc::now(), false)
        .await
        .expect("watch");
    // Its warning window (expiry − 120 min) began before this device started watching: listed, read.
    assert_eq!(
        (r.raised, r.already_true),
        (0, 1),
        "120 min lead time covers a permit expiring in 60 min"
    );
    exec(
        &db,
        "UPDATE notification_rules SET lead_time_minutes = 30 WHERE category_code = 'ptw_expiring'",
        vec![],
    )
    .await;
    let r = watchers::run_watcher(&db, "ptw_expiring", Utc::now(), false)
        .await
        .expect("watch");
    assert_eq!(r.resolved, 1, "outside a 30 min lead time");
}

// ─── Review follow-ups ─────────────────────────────────────────────────────────

async fn org_node(db: &DatabaseConnection, id: i64, parent: Option<i64>) {
    exec(
        db,
        &format!(
            "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, parent_id, status, created_at, \
             updated_at, row_version, structure_model_id) VALUES (?, ?, ?, ?, 1, ?, 'active', {NOW}, {NOW}, 1, 1)"
        ),
        vec![
            id.into(),
            format!("nt-node-{id}").into(),
            format!("N-{id}").into(),
            format!("Node {id}").into(),
            parent.into(),
        ],
    )
    .await;
}

/// A role holding `permission`, granted to `user_id` on one org node only.
async fn grant_on_node(db: &DatabaseConnection, user_id: i64, permission: &str, node: i64) {
    let role = format!("nt-node-role-{user_id}");
    exec(
        db,
        &format!(
            "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at) \
             VALUES (?, ?, '', 0, 'custom', 'active', {NOW}, {NOW})"
        ),
        vec![role.clone().into(), role.clone().into()],
    )
    .await;
    let role_id = scalar_i64(db, "SELECT id FROM roles WHERE name = ?", vec![role.into()]).await;
    exec(
        db,
        &format!("INSERT INTO role_permissions (role_id, permission_id, granted_at) SELECT ?, id, {NOW} FROM permissions WHERE name = ?"),
        vec![role_id.into(), permission.into()],
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
             created_at, updated_at) VALUES (?, ?, ?, 'org_node', ?, {NOW}, {NOW})"
        ),
        vec![
            format!("nt-usa-node-{user_id}").into(),
            user_id.into(),
            role_id.into(),
            node.to_string().into(),
        ],
    )
    .await;
}

/// The device has been watching every rule since yesterday.
async fn watching_since_yesterday(db: &DatabaseConnection) {
    exec(
        db,
        "UPDATE notification_watch_state SET watching_since = ?",
        vec![past(24 * 60).into()],
    )
    .await;
}

#[tokio::test]
async fn a_view_permission_granted_on_a_site_shows_the_notifications_of_that_site_and_below() {
    let db = db().await;
    org_node(&db, 901, None).await;
    org_node(&db, 902, Some(901)).await;
    org_node(&db, 903, None).await;
    user(&db, 90, None).await;
    grant_on_node(&db, 90, crate::rbac::permissions::OT_VIEW, 901).await;
    tick(&db).await;
    watching_since_yesterday(&db).await;
    for (id, node) in [(21, Some(902_i64)), (22, Some(903)), (23, None)] {
        work_order(&db, id, Some(90), None, None).await;
        exec(
            &db,
            "UPDATE work_orders SET entity_id = ? WHERE id = ?",
            vec![node.into(), id.into()],
        )
        .await;
    }
    tick(&db).await;
    assert_eq!(notifications_of(&db, 90, "wo_assigned").await, 3);

    let vis = inbox::visible_to(&db, 90).await.expect("visibility");
    assert!(vis.everywhere.iter().all(|m| m != "ot"), "not plant-wide");
    assert!(vis.sees_module("ot"));
    let items = inbox::list(&db, 90, &vis, &InboxFilter::default()).await.expect("list");
    let mut codes: Vec<String> = items
        .iter()
        .filter_map(|i| {
            i.message_params
                .as_ref()
                .and_then(|p| p["code"].as_str().map(str::to_string))
        })
        .collect();
    codes.sort();
    // The area below the granted site; not the other site, nor the order without a site: a work
    // order without a site is a plant-wide record its list never shows a site user (AU-112).
    assert_eq!(codes, vec!["OT-21".to_string()]);
    assert_eq!(
        inbox::unread_count(&db, 90, &vis).await.expect("bell"),
        1,
        "bell = inbox"
    );
    let no_site = scalar_i64(
        &db,
        "SELECT n.id FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.source_record_id = '23'",
        vec![],
    )
    .await;
    assert!(inbox::ensure_visible(&db, 90, &vis, no_site).await.is_err());
    let other_site = scalar_i64(
        &db,
        "SELECT n.id FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.source_record_id = '22'",
        vec![],
    )
    .await;
    assert!(inbox::ensure_visible(&db, 90, &vis, other_site).await.is_err());
}

#[tokio::test]
async fn conditions_already_true_when_watching_starts_are_listed_read_never_swallowed() {
    let db = db().await;
    user(&db, 100, None).await;
    exec(
        &db,
        "UPDATE notification_rules SET requires_ack = 1 WHERE category_code = 'wo_overdue'",
        vec![],
    )
    .await;
    // A new device: the order was assigned and overdue before it started watching.
    work_order(&db, 31, Some(100), None, Some(&past(120))).await;
    exec(
        &db,
        "UPDATE work_orders SET updated_at = ? WHERE id = 31",
        vec![past(180).into()],
    )
    .await;
    tick(&db).await;
    let modules = all_modules();
    assert_eq!(notifications_of(&db, 100, "wo_overdue").await, 1, "listed");
    assert_eq!(
        inbox::unread_count(&db, 100, &modules).await.expect("bell"),
        0,
        "no bell"
    );
    let items = inbox::list(&db, 100, &modules, &InboxFilter::default())
        .await
        .expect("list");
    let overdue = items.iter().find(|i| i.category_code == "wo_overdue").expect("overdue");
    assert!(overdue.already_true && overdue.read_at.is_some());
    let to_ack = InboxFilter {
        view: Some("to_acknowledge".into()),
        ..InboxFilter::default()
    };
    assert_eq!(
        inbox::list(&db, 100, &modules, &to_ack).await.expect("list").len(),
        1,
        "still to acknowledge"
    );
    let toasts = crate::notifications::os_toast::pending_os_toasts(&db, 100, Some(0))
        .await
        .expect("toasts");
    assert!(toasts.items.is_empty(), "no toast either");

    // Once watching, a new overdue order rings the bell.
    watching_since_yesterday(&db).await;
    work_order(&db, 32, Some(100), None, Some(&past(10))).await;
    tick(&db).await;
    let fresh = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'wo_overdue' AND n.read_at IS NULL AND n.already_true = 0",
        vec![],
    )
    .await;
    assert_eq!(fresh, 1);

    // An inactive rule is not watched; activated later, what became true meanwhile is listed read.
    exec(
        &db,
        "UPDATE notification_rules SET is_active = 0 WHERE category_code = 'wo_overdue'",
        vec![],
    )
    .await;
    tick(&db).await;
    work_order(&db, 33, Some(100), None, Some(&past(5))).await;
    tick(&db).await;
    assert_eq!(
        notifications_of(&db, 100, "wo_overdue").await,
        2,
        "nothing while inactive"
    );
    exec(
        &db,
        "UPDATE notification_rules SET is_active = 1 WHERE category_code = 'wo_overdue'",
        vec![],
    )
    .await;
    tick(&db).await;
    let late = scalar_i64(
        &db,
        "SELECT n.already_true FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'wo_overdue' AND e.source_record_id = '33'",
        vec![],
    )
    .await;
    assert_eq!(late, 1);
}

#[tokio::test]
async fn a_failing_stage_does_not_stop_the_other_stages() {
    let db = db().await;
    user(&db, 110, None).await;
    tick(&db).await;
    watching_since_yesterday(&db).await;
    work_order(&db, 41, Some(110), None, Some(&past(10))).await;
    // One watcher cannot read its table on this device (private in-memory test database).
    exec(
        &db,
        "ALTER TABLE inspection_rounds RENAME TO inspection_rounds_gone",
        vec![],
    )
    .await;
    scheduler::run_scheduler_tick_for_test(&db).await.expect("tick");
    assert_eq!(
        notifications_of(&db, 110, "wo_overdue").await,
        1,
        "earlier stage committed"
    );
    assert_eq!(notifications_of(&db, 110, "wo_assigned").await, 1);
    // A later stage still runs: a snoozed notification comes back.
    let id = scalar_i64(
        &db,
        "SELECT n.id FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'wo_assigned' AND n.recipient_user_id = 110",
        vec![],
    )
    .await;
    exec(
        &db,
        "UPDATE notifications SET delivery_state = 'snoozed', snoozed_until = ? WHERE id = ?",
        vec![past(1).into(), id.into()],
    )
    .await;
    scheduler::run_scheduler_tick_for_test(&db).await.expect("tick");
    let state = scalar_opt_text(
        &db,
        "SELECT delivery_state FROM notifications WHERE id = ?",
        vec![id.into()],
    )
    .await;
    assert_eq!(state.as_deref(), Some("delivered"));
}

#[tokio::test]
async fn elevated_admins_hold_every_permission_for_routing_too() {
    let db = db().await;
    user(&db, 120, None).await;
    exec(&db, "UPDATE user_accounts SET is_admin = 1 WHERE id = 120", vec![]).await;
    let holders =
        crate::notifications::router::permission_holder_user_ids(&db, crate::rbac::permissions::INV_MANAGE, None)
            .await
            .expect("holders");
    assert!(holders.contains(&120));
    let admins = crate::auth::rbac::elevated_admin_user_ids(&db).await.expect("admins");
    assert!(admins.contains(&120));
    for a in &admins {
        let id = i32::try_from(*a).expect("id");
        assert!(
            crate::auth::rbac::user_has_elevated_admin_access(&db, id)
                .await
                .expect("check"),
            "same rule as the permission checks"
        );
    }
}

#[tokio::test]
async fn a_stock_occurrence_is_identified_by_the_movement_that_crossed_the_reorder_point() {
    let db = db().await;
    exec(
        &db,
        &format!(
            "INSERT INTO articles (id, article_code, article_name, unit_value_id, reorder_point, is_active, entity_sync_id, \
             created_at, updated_at) VALUES (910, 'ART-910', 'Seal', 1, 5, 1, 'nt-art-910', {NOW}, {NOW})"
        ),
        vec![],
    )
    .await;
    for (sid, kind, qty, at) in [
        ("nt-tx-1", "ADJUST_IN", 10.0, "2026-10-01T08:00:00Z"),
        ("nt-tx-2", "ADJUST_OUT", 3.0, "2026-10-02T08:00:00Z"),
        ("nt-tx-3", "ADJUST_OUT", 4.0, "2026-10-03T08:00:00Z"),
    ] {
        exec(
            &db,
            "INSERT INTO inventory_transactions (entity_sync_id, article_id, warehouse_id, location_id, movement_type, \
             quantity, source_type, performed_at) VALUES (?, 910, 1, 1, ?, ?, 'manual', ?)",
            vec![sid.into(), kind.into(), qty.into(), at.into()],
        )
        .await;
    }
    exec(
        &db,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, available_qty) \
         VALUES (910, 1, 1, 3, 3)",
        vec![],
    )
    .await;
    let c = watchers::candidates(&db, "stock_critical", Utc::now(), None, false)
        .await
        .expect("candidates");
    let seal = c
        .iter()
        .find(|c| c.record_key.starts_with("nt-art-910|"))
        .expect("seal");
    // 10 → 7 stays above 5; 7 → 3 crosses it: the occurrence every device names the same way.
    assert_eq!(seal.record_key, "nt-art-910|nt-tx-3");
    assert_eq!(seal.since, watchers::parse_time("2026-10-03T08:00:00Z"));
}

/// AU-112: a request notice without a node is plant-wide, like a work order without a site: a
/// reviewer granted di.view on a site does not see it; once granted di.view plant-wide, they do.
#[tokio::test]
async fn a_request_without_a_site_is_not_shown_to_a_site_reviewer() {
    let db = db().await;
    org_node(&db, 911, None).await;
    user(&db, 91, None).await;
    grant_on_node(&db, 91, crate::rbac::permissions::DI_VIEW, 911).await;
    for id in [311_i64, 312] {
        exec(
            &db,
            &format!(
                "INSERT INTO intervention_requests (id, code, equipment_id, org_node_id, status, title, description, \
                 origin_type, impact_level, production_impact, safety_flag, environmental_flag, quality_flag, \
                 reported_urgency, observed_at, submitted_at, reviewer_id, submitter_id, entity_sync_id, created_at, \
                 updated_at, row_version) \
                 VALUES (?, ?, 1, ?, 'in_review', 'Leak', 'Oil leak', 'operator', 'minor', 0, 0, 0, 0, 'high', {NOW}, \
                 {NOW}, 91, 91, ?, {NOW}, {NOW}, 1)"
            ),
            vec![
                id.into(),
                format!("DI-{id}").into(),
                911_i64.into(),
                format!("nt-di-{id}").into(),
            ],
        )
        .await;
        emitter::emit_event(
            &db,
            emitter::NotificationEventInput {
                source_module: "di".into(),
                source_record_id: Some(id.to_string()),
                event_code: "di_entered_in_review".into(),
                category_code: "di_pending_review".into(),
                severity: "info".into(),
                dedupe_key: Some(format!("di-review-{id}")),
                payload_json: None,
                title: format!("DI en revue — DI-{id}"),
                body: None,
                action_url: None,
            },
        )
        .await
        .expect("emit");
    }
    // A request row always has a node: the notice of 312 is made one without a node, as a
    // notice raised with no site would be.
    exec(
        &db,
        "UPDATE notification_events SET org_node_id = NULL WHERE source_record_id = '312'",
        vec![],
    )
    .await;
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.source_record_id = '312' AND n.recipient_user_id = 91",
            vec![],
        )
        .await,
        1,
        "the site reviewer was notified: only the inbox hides it"
    );
    let records = |items: Vec<inbox::InboxItem>| {
        let mut ids: Vec<String> = items.into_iter().filter_map(|i| i.source_record_id).collect();
        ids.sort();
        ids.dedup();
        ids
    };
    let site_vis = inbox::visible_to(&db, 91).await.expect("visibility");
    let site_items = inbox::list(&db, 91, &site_vis, &InboxFilter::default())
        .await
        .expect("list");
    assert_eq!(records(site_items), vec!["311".to_string()], "the site's request only");
    grant(&db, 91, &[crate::rbac::permissions::DI_VIEW]).await;
    let plant_vis = inbox::visible_to(&db, 91).await.expect("visibility");
    let plant_items = inbox::list(&db, 91, &plant_vis, &InboxFilter::default())
        .await
        .expect("list");
    assert_eq!(
        records(plant_items),
        vec!["311".to_string(), "312".to_string()],
        "plant-wide: the request notice without a node too"
    );
}
