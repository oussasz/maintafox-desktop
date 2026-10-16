//! Inbox, subscriptions, watchers, resolution and escalation, on a seeded database.

use chrono::{Duration, Utc};
use sea_orm::DatabaseConnection;
use tokio::sync::RwLock;

use crate::notifications::catalog::MODULES;
use crate::notifications::inbox::{self, InboxFilter};
use crate::notifications::{conditions, config, delivery, emitter, scheduler, watchers};
use crate::rbac::cache::PermissionCache;
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

fn all_modules() -> Vec<String> {
    MODULES.iter().map(|(m, _)| (*m).to_string()).collect()
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
    // Watchers start silently, then the new assignment is notified.
    tick(&db).await;
    work_order(&db, 2, Some(10), None, None).await;
    tick(&db).await;
    assert_eq!(
        notifications_of(&db, 10, "wo_assigned").await,
        1,
        "only the new assignment"
    );

    let cache = RwLock::new(PermissionCache::default());
    let visible = inbox::visible_modules(&db, &cache, 10).await.expect("modules");
    assert!(visible.contains(&"di".to_string()) && !visible.contains(&"ot".to_string()));
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
        1
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
    config::add_step(&db, policy.id, &step(240, "permission", Some("adm.settings")))
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
    assert_eq!(r.raised, 1, "120 min lead time covers a permit expiring in 60 min");
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
