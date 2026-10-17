//! Home page panels on a real migrated + seeded in-memory database: every figure follows the
//! owning module's rule, sections follow the person's access, nothing fails silently to 0.

use chrono::{NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::my_work::{build_my_work, my_pm, my_requests, my_rounds, my_work_orders, MyWorkAccess};
use super::planning::{build_planning, NO_FROZEN_BASELINE, WEEK_NOT_FROZEN, WEEK_NO_PLANNING_SITE};
use super::supervision::{assets, build_supervision, permit_list, work_orders, SupervisionAccess};
use super::{checked_period, period_first_day, period_window, urgency_rank_sql};
use crate::commands::dashboard::{build_workload_chart, WorkloadSeries};

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
        .await
        .expect("equipment taxonomy");
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'hm-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'hm-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'hm-node-1', 'SITE-A', 'Site A', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
    ] {
        exec(&db, sql).await;
    }
    db
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<i64>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

async fn other_user(db: &DatabaseConnection) -> i64 {
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, \
         is_admin, force_password_change, created_at, updated_at) \
         VALUES ('hm-user-other', 'other', 'Other', 'local', 1, 0, 0, \
         strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
    )
    .await;
    scalar(db, "SELECT id FROM user_accounts WHERE username = 'other'").await
}

async fn equipment(db: &DatabaseConnection, code: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('hm-eq-{code}', '{code}', 'Asset {code}', 'active_in_service', 1, \
             datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
}

async fn ref_id(db: &DatabaseConnection, domain: &str, code: &str) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = '{domain}' AND rs.status = 'published' AND rv.code = '{code}'"
        ),
    )
    .await
}

async fn type_id(db: &DatabaseConnection, code: &str) -> i64 {
    ref_id(db, "WORK.ORDER_TYPES", code).await
}

/// One work order; `planned_*` are SQL expressions (or `NULL`).
#[allow(clippy::too_many_arguments)]
async fn work_order(
    db: &DatabaseConnection,
    code: &str,
    type_code: &str,
    status: &str,
    responsible: i64,
    urgency: Option<&str>,
    planned_start: &str,
    planned_end: &str,
) -> i64 {
    let t = type_id(db, type_code).await;
    let u = match urgency {
        Some(c) => crate::reference::urgency_scale::urgency_value_id(db, c)
            .await
            .expect("urgency")
            .to_string(),
        None => "NULL".to_string(),
    };
    exec(
        db,
        &format!(
            "INSERT INTO work_orders (code, type_id, status_id, entity_id, title, primary_responsible_id, urgency_id, \
             planned_start, planned_end, row_version, created_at, updated_at) \
             VALUES ('{code}', {t}, (SELECT id FROM work_order_statuses WHERE code = '{status}'), 1, '{code}', \
             {responsible}, {u}, {planned_start}, {planned_end}, 1, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    scalar(db, &format!("SELECT id FROM work_orders WHERE code = '{code}'")).await
}

async fn request(db: &DatabaseConnection, code: &str, status: &str, submitter: i64, converted: bool) {
    let eq = equipment(db, &format!("DI-EQ-{code}")).await;
    let converted_at = if converted {
        "datetime('now', '-1 days')"
    } else {
        "NULL"
    };
    let closed_at = if status == "closed" {
        "datetime('now', '-1 days')"
    } else {
        "NULL"
    };
    exec(
        db,
        &format!(
            "INSERT INTO intervention_requests (code, equipment_id, org_node_id, status, title, description, \
             origin_type, impact_level, reported_urgency, submitted_at, submitter_id, closed_at, converted_at, \
             created_at, updated_at) VALUES ('{code}', {eq}, 1, '{status}', '{code}', 'd', 'operator', 'unknown', \
             'normal', datetime('now', '-2 days'), {submitter}, {closed_at}, {converted_at}, \
             datetime('now', '-2 days'), datetime('now'))"
        ),
    )
    .await;
}

// ── Pure rules ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_period_selector_accepts_only_its_choices() {
    assert_eq!(checked_period(7).expect("7"), 7);
    assert_eq!(checked_period(30).expect("30"), 30);
    assert!(checked_period(14).is_err(), "never silently replaced by a default");
    assert!(checked_period(0).is_err());
}

#[test]
fn urgency_rank_follows_the_shared_scale() {
    let sql = urgency_rank_sql("ul.code");
    assert!(sql.starts_with("CASE ul.code"));
    assert!(sql.contains("WHEN 'emergency' THEN 1"));
    assert!(sql.contains("WHEN 'low' THEN 5"));
    assert!(sql.ends_with("ELSE 99 END"));
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("date")
}

#[test]
fn the_period_is_the_last_days_today_included() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-16T09:30:00Z")
        .expect("now")
        .with_timezone(&Utc);
    assert_eq!(period_first_day(now, 7), date(2026, 10, 10));
    let w = period_window(now, 30);
    assert_eq!(w.from.to_rfc3339(), "2026-09-17T00:00:00+00:00");
    assert_eq!(w.to, now);
}

/// The planner panel's week load and compliance are the planning KPIs' (`planning::kpis`, measured
/// against the frozen weekly baseline — proved by `planning::tests::planning_kpis_measure_the_frozen_baseline`).
/// Without a planning site or a frozen week they give their reason, never a number.
#[tokio::test]
async fn the_planner_panel_says_why_when_there_is_no_frozen_baseline() {
    let db = setup().await;
    let now = Utc::now();
    let panel = build_planning(&db, 30, now).await.expect("panel");
    assert!(panel.week.is_none());
    assert_eq!(panel.week_unavailable.as_deref(), Some(WEEK_NO_PLANNING_SITE));
    assert_eq!(panel.schedule_compliance.value_pct, None);
    assert_eq!(
        panel.schedule_compliance.unavailable.as_deref(),
        Some(NO_FROZEN_BASELINE)
    );

    // A planning site with a crew, its week never frozen: still no number.
    exec(&db, "UPDATE org_node_types SET can_own_work = 1 WHERE id = 1").await;
    exec(
        &db,
        "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at) \
         VALUES ('hm-crew', 'HM-CREW', 'Home crew', 'maintenance', 1, 'active', datetime('now'), datetime('now'))",
    )
    .await;
    let panel = build_planning(&db, 30, now).await.expect("panel");
    assert!(panel.week.is_none());
    assert_eq!(panel.week_unavailable.as_deref(), Some(WEEK_NOT_FROZEN));
    assert_eq!(
        panel.schedule_compliance.unavailable.as_deref(),
        Some(NO_FROZEN_BASELINE)
    );
}

// ── My work ────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn my_work_orders_are_my_open_work_overdue_first_then_most_urgent() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let other = other_user(&db).await;
    let now = Utc::now();
    work_order(
        &db,
        "WO-LATE",
        "corrective",
        "in_progress",
        me,
        Some("normal"),
        "strftime('%Y-%m-%dT%H:%M:%SZ','now','-3 days')",
        "strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 days')",
    )
    .await;
    work_order(
        &db,
        "WO-TODAY",
        "emergency",
        "ready",
        me,
        Some("emergency"),
        "strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        "strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 minutes')",
    )
    .await;
    work_order(
        &db,
        "WO-NEXT",
        "preventive",
        "planning",
        me,
        None,
        "strftime('%Y-%m-%dT%H:%M:%SZ','now','+5 days')",
        "strftime('%Y-%m-%dT%H:%M:%SZ','now','+6 days')",
    )
    .await;
    // Not my open work: done, cancelled, someone else's.
    work_order(&db, "WO-DONE", "corrective", "completed", me, None, "NULL", "NULL").await;
    work_order(&db, "WO-CANC", "corrective", "cancelled", me, None, "NULL", "NULL").await;
    work_order(
        &db,
        "WO-OTHER",
        "corrective",
        "in_progress",
        other,
        None,
        "NULL",
        "NULL",
    )
    .await;

    let w = my_work_orders(&db, me, now).await.expect("my work orders");
    assert_eq!((w.open, w.overdue, w.today), (3, 1, 1));
    let codes: Vec<&str> = w.items.iter().map(|i| i.code.as_str()).collect();
    assert_eq!(codes, ["WO-LATE", "WO-TODAY", "WO-NEXT"]);
    assert!(w.items[0].overdue && !w.items[1].overdue);
    let urgencies: Vec<(Option<&str>, i64)> = w
        .by_urgency
        .iter()
        .map(|u| (u.urgency_code.as_deref(), u.count))
        .collect();
    assert_eq!(urgencies, [(Some("emergency"), 1), (Some("normal"), 1), (None, 1)]);
}

#[tokio::test]
async fn my_requests_are_the_di_queue_mine() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let other = other_user(&db).await;
    request(&db, "DI-H1", "submitted", me, false).await;
    request(&db, "DI-H2", "returned_for_clarification", me, false).await;
    request(&db, "DI-H3", "closed", me, true).await;
    request(&db, "DI-H4", "submitted", other, false).await;
    let r = my_requests(&db, me).await.expect("my requests");
    let queues = crate::di::queries::count_di_queues(&db, me).await.expect("queues");
    assert_eq!((r.open, r.awaiting_my_answer), (queues.mine, queues.mine_awaiting_info));
    assert_eq!((r.open, r.awaiting_my_answer), (2, 1));
}

#[tokio::test]
async fn a_login_without_personnel_has_no_rounds_to_do() {
    let db = setup().await;
    let me = admin_id(&db).await;
    exec(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = NULL WHERE id = {me}"),
    )
    .await;
    let r = my_rounds(&db, me, Utc::now()).await.expect("rounds");
    assert!(!r.personnel_linked);
    assert_eq!((r.overdue, r.today, r.next_7_days, r.items.len()), (0, 0, 0, 0));
}

#[tokio::test]
async fn my_pm_follows_the_pm_page_overdue_rule() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let eq = equipment(&db, "PM-EQ").await;
    let plan = crate::pm::queries::create_pm_plan(
        &db,
        crate::pm::domain::CreatePmPlanInput {
            code: "PM-HOME".into(),
            title: "Lubrication".into(),
            description: None,
            asset_scope_type: "equipment".into(),
            asset_scope_id: Some(eq),
            strategy_type: "event".into(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("pm plan");
    crate::pm::queries::create_pm_plan_version(
        &db,
        plan.id,
        crate::pm::domain::CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
            trigger_definition_json: r#"{"event_code":"INSPECTION_COMPLETED"}"#.into(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".into()),
            required_tools_json: None,
            estimated_duration_hours: Some(1.0),
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("pm version");
    let wo = work_order(&db, "WO-PM", "preventive", "ready", me, None, "NULL", "NULL").await;
    // Due 3 days ago with a window still open tomorrow: not overdue (the PM page's rule);
    // due 2 days ago without a window: overdue; due in 2 days: due within 7 days.
    for (due, window_end) in [
        (
            "strftime('%Y-%m-%dT%H:%M:%SZ','now','-3 days')",
            "strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 days')",
        ),
        ("strftime('%Y-%m-%dT%H:%M:%SZ','now','-2 days')", "NULL"),
        ("strftime('%Y-%m-%dT%H:%M:%SZ','now','+2 days')", "NULL"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO pm_occurrences (pm_plan_id, plan_version_id, due_basis, due_at, window_end_at, status, \
                 linked_work_order_id) VALUES ({p}, (SELECT id FROM pm_plan_versions WHERE pm_plan_id = {p} \
                 ORDER BY id LIMIT 1), 'calendar', {due}, {window_end}, 'forecasted', {wo})",
                p = plan.id
            ),
        )
        .await;
    }
    let pm = my_pm(&db, me, Utc::now()).await.expect("my pm");
    assert_eq!((pm.overdue, pm.due_next_7_days), (1, 1));
    assert_eq!(pm.items.len(), 3);
    assert!(pm.items[0].overdue, "overdue first");
    assert!(pm.items.iter().all(|i| i.work_order_code == "WO-PM"));
}

#[tokio::test]
async fn a_section_the_person_may_not_open_is_absent() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let w = build_my_work(&db, me as i32, MyWorkAccess::default(), Utc::now())
        .await
        .expect("my work");
    assert!(w.work_orders.is_none() && w.requests.is_none() && w.rounds.is_none() && w.pm.is_none());
    assert_eq!((w.notifications.unread, w.notifications.to_acknowledge), (0, 0));

    let s = build_supervision(&db, me as i32, SupervisionAccess::default(), 7, Utc::now())
        .await
        .expect("supervision");
    assert!(s.requests.is_none() && s.work_orders.is_none() && s.pm.is_none());
    assert!(s.stock.is_none() && s.safety.is_none() && s.assets.is_none());
}

// ── Supervision ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ot_backlog_splits_by_nature_and_urgency() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let late = "strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 days')";
    work_order(
        &db,
        "B-EMG",
        "emergency",
        "in_progress",
        me,
        Some("emergency"),
        "NULL",
        late,
    )
    .await;
    work_order(&db, "B-COR", "corrective", "ready", me, Some("high"), "NULL", "NULL").await;
    work_order(
        &db,
        "B-PRV",
        "preventive",
        "planning",
        me,
        Some("normal"),
        "NULL",
        "NULL",
    )
    .await;
    work_order(&db, "B-IMP", "improvement", "draft", me, None, "NULL", "NULL").await;
    work_order(&db, "B-DONE", "corrective", "completed", me, None, "NULL", late).await;
    work_order(&db, "B-CLOSED", "corrective", "closed", me, None, "NULL", late).await;

    let w = work_orders(&db, Utc::now()).await.expect("backlog");
    assert_eq!(w.open, 4, "draft to on hold only");
    assert_eq!((w.planned, w.reactive, w.emergency, w.without_nature), (2, 2, 1, 0));
    // One overdue rule: work still ahead and planned end past — B-EMG only (B-DONE is completed,
    // B-CLOSED closed). The OT dashboard and the as-of count agree.
    assert_eq!(w.overdue, 1);
    assert_eq!(
        w.overdue,
        crate::analytics::as_of::wo_overdue_at(&db, Utc::now(), None)
            .await
            .expect("as of")
    );
    assert_eq!(
        w.overdue,
        crate::wo::stats::get_wo_stats(&db).await.expect("stats").overdue
    );
    let emergency = w
        .cells
        .iter()
        .find(|c| c.nature_code.as_deref() == Some("corrective_immediate"))
        .expect("emergency cell");
    assert_eq!(
        (emergency.urgency_code.as_deref(), emergency.count),
        (Some("emergency"), 1)
    );
}

#[tokio::test]
async fn critical_assets_down_are_read_from_reference_data() {
    let db = setup().await;
    let a = ref_id(&db, "EQUIPMENT.CRITICALITY", "A").await;
    let b = ref_id(&db, "EQUIPMENT.CRITICALITY", "B").await;
    let down = ref_id(&db, "EQUIPMENT.STATUS", "OUT_OF_SERVICE").await;
    let maint = ref_id(&db, "EQUIPMENT.STATUS", "UNDER_MAINTENANCE").await;
    let up = ref_id(&db, "EQUIPMENT.STATUS", "ACTIVE_IN_SERVICE").await;
    for (code, crit, status) in [
        ("CR-1", a, down),
        ("CR-2", a, maint),
        ("CR-3", a, up),
        ("CR-4", b, down),
    ] {
        let id = equipment(&db, code).await;
        exec(
            &db,
            &format!(
                "UPDATE equipment SET equipment_criticality_ref_id = {crit}, equipment_status_ref_id = {status} \
                 WHERE id = {id}"
            ),
        )
        .await;
    }
    exec(
        &db,
        "UPDATE equipment SET deleted_at = datetime('now') WHERE asset_id_code = 'CR-2'",
    )
    .await;

    let s = assets(&db).await.expect("assets");
    assert_eq!(
        s.critical_down, 1,
        "CR-1 only: CR-2 deleted, CR-3 in service, CR-4 not top level"
    );
    assert_eq!(
        s.items.iter().map(|i| i.asset_code.as_str()).collect::<Vec<_>>(),
        ["CR-1"]
    );
    assert_eq!(s.criticality_label.as_deref(), Some("Criticité A"));
    assert_eq!(s.down_status_labels, ["Hors service", "En maintenance"]);

    // The most critical level is the first of the list, whatever its code.
    exec(
        &db,
        &format!("UPDATE reference_values SET sort_order = 0 WHERE id = {b}"),
    )
    .await;
    let s = assets(&db).await.expect("assets");
    assert_eq!(
        s.items.iter().map(|i| i.asset_code.as_str()).collect::<Vec<_>>(),
        ["CR-4"]
    );
}

#[tokio::test]
async fn open_requests_as_of_now_are_the_di_module_open_requests() {
    let db = setup().await;
    let me = admin_id(&db).await;
    request(&db, "DI-O1", "submitted", me, false).await;
    request(&db, "DI-O2", "in_review", me, false).await;
    request(&db, "DI-O3", "returned_for_clarification", me, false).await;
    request(&db, "DI-O4", "approved", me, false).await;
    request(&db, "DI-O5", "closed", me, true).await;
    request(&db, "DI-O6", "closed", me, false).await;
    let module = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM intervention_requests ir WHERE {}",
            crate::di::stats::OPEN_SQL
        ),
    )
    .await;
    let as_of = crate::analytics::as_of::di_open_at(&db, Utc::now(), None, None)
        .await
        .expect("as of");
    assert_eq!(module, 4);
    assert_eq!(as_of, module);
}

// ── Activity chart ─────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn activity_chart_is_one_row_per_day_and_hides_forbidden_series() {
    let db = setup().await;
    let me = admin_id(&db).await;
    request(&db, "DI-W1", "submitted", me, false).await;
    let chart = build_workload_chart(
        &db,
        7,
        WorkloadSeries {
            di: true,
            wo: false,
            pm: true,
        },
    )
    .await
    .expect("chart");
    assert_eq!(chart.days.len(), 7);
    assert!(
        chart.days.iter().all(|d| d.wo_completed.is_none()),
        "no OT permission: no series"
    );
    assert!(chart.days.iter().all(|d| d.di_created.is_some() && d.pm_due.is_some()));
    let submitted: i64 = chart.days.iter().filter_map(|d| d.di_created).sum();
    assert_eq!(submitted, 1, "submitted two days ago, inside the week");
    assert_eq!(chart.quality_badge, None);
    let dates: Vec<&str> = chart.days.iter().map(|d| d.date.as_str()).collect();
    let mut sorted = dates.clone();
    sorted.sort_unstable();
    assert_eq!(dates, sorted);

    assert!(build_workload_chart(&db, 14, WorkloadSeries::default()).await.is_err());
}

#[test]
fn the_overdue_rule_stays_inside_the_open_statuses() {
    let open = crate::wo::backlog::OPEN_STATUS_SQL;
    assert!(
        crate::wo::backlog::WO_OVERDUE_SQL.contains(&format!("s.code IN {open}")),
        "overdue must be part of the work still ahead"
    );
}

#[tokio::test]
async fn a_request_stops_being_open_at_its_conversion_when_it_comes_first() {
    let db = setup().await;
    let me = admin_id(&db).await;
    // Converted 10 days ago, closed (status) 2 days ago: open until the conversion only.
    request(&db, "DI-C1", "closed", me, false).await;
    exec(
        &db,
        "UPDATE intervention_requests SET submitted_at = datetime('now', '-20 days'), \
         converted_at = datetime('now', '-10 days'), closed_at = datetime('now', '-2 days') WHERE code = 'DI-C1'",
    )
    .await;
    let at = |d: i64| Utc::now() + chrono::Duration::days(d);
    let open_at = |d: i64| crate::analytics::as_of::di_open_at(&db, at(d), None, None);
    assert_eq!(open_at(-15).await.expect("−15"), 1, "submitted, not yet converted");
    assert_eq!(
        open_at(-5).await.expect("−5"),
        0,
        "converted at −10 d (before its closing)"
    );
    assert_eq!(open_at(0).await.expect("now"), 0);
}

fn candidate(record_id: Option<i64>, code: &str) -> crate::notifications::watchers::Candidate {
    crate::notifications::watchers::Candidate {
        record_id,
        record_key: code.into(),
        since: None,
        source_module: "permit",
        message_key: "ptw_expiring",
        params: serde_json::json!({ "code": code, "title": "Hot work", "asset": "P-1", "expires_at": "2026-10-16T18:00:00Z" }),
        urgency: None,
        action_url: String::new(),
        severity: "warning".into(),
        title: String::new(),
        body: String::new(),
    }
}

#[test]
fn a_permit_list_counts_everything_and_keeps_the_first_ones() {
    let mut found: Vec<_> = (1..=8).map(|i| candidate(Some(i), &format!("PTW-{i}"))).collect();
    found.push(candidate(None, "PTW-X"));
    let list = permit_list(found);
    assert_eq!(list.total, 9, "every permit found is counted");
    assert_eq!(list.without_record, 1, "one cannot be opened: counted, not dropped");
    assert_eq!(list.items.len(), super::ITEM_LIMIT as usize);
    assert_eq!(list.items[0].code, "PTW-1");
}

// ── List drills: a KPI opens the list of exactly what it counted ──────────────────────────────

fn wo_scope(open_only: bool, overdue_only: bool) -> crate::wo::queries::WoListFilter {
    crate::wo::queries::WoListFilter {
        open_only: Some(open_only),
        overdue_only: Some(overdue_only),
        limit: 200,
        offset: 0,
        ..Default::default()
    }
}

#[tokio::test]
async fn the_ot_list_scopes_list_what_the_open_and_overdue_counts_count() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let late = "strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 days')";
    let later = "strftime('%Y-%m-%dT%H:%M:%SZ','now','+3 days')";
    work_order(&db, "L-LATE", "corrective", "in_progress", me, None, "NULL", late).await;
    work_order(&db, "L-HOLD", "corrective", "on_hold", me, None, "NULL", late).await;
    work_order(&db, "L-AHEAD", "corrective", "ready", me, None, "NULL", later).await;
    work_order(&db, "L-NOEND", "preventive", "draft", me, None, "NULL", "NULL").await;
    work_order(&db, "L-DONE", "corrective", "completed", me, None, "NULL", late).await;
    work_order(&db, "L-CLOSED", "corrective", "closed", me, None, "NULL", late).await;

    let w = work_orders(&db, Utc::now()).await.expect("backlog");
    let open = crate::wo::queries::list_work_orders(&db, wo_scope(true, false))
        .await
        .expect("open list");
    assert_eq!(open.total, w.open);
    assert_eq!(open.total, 4);

    let overdue = crate::wo::queries::list_work_orders(&db, wo_scope(false, true))
        .await
        .expect("overdue list");
    assert_eq!(overdue.total, w.overdue);
    assert_eq!(
        overdue.total,
        crate::wo::stats::get_wo_stats(&db).await.expect("stats").overdue
    );
    let mut codes: Vec<&str> = overdue.items.iter().map(|x| x.code.as_str()).collect();
    codes.sort_unstable();
    assert_eq!(codes, ["L-HOLD", "L-LATE"]);
}

fn di_scope(open_only: bool, waiting_days: Option<i64>) -> crate::di::queries::DiListFilter {
    crate::di::queries::DiListFilter {
        open_only: Some(open_only),
        waiting_days,
        limit: 200,
        offset: 0,
        ..Default::default()
    }
}

#[tokio::test]
async fn the_di_list_scopes_list_what_the_open_and_waiting_counts_count() {
    let db = setup().await;
    let me = admin_id(&db).await;
    request(&db, "W-NEW", "submitted", me, false).await; // 2 days
    request(&db, "W-OLD", "in_review", me, false).await;
    request(&db, "W-OLD-CLOSED", "closed", me, false).await;
    request(&db, "W-OLD-CONV", "approved", me, true).await;
    request(&db, "W-OLD-INFO", "returned_for_clarification", me, false).await;
    exec(
        &db,
        "UPDATE intervention_requests SET submitted_at = datetime('now', '-10 days') \
         WHERE code IN ('W-OLD', 'W-OLD-CLOSED', 'W-OLD-CONV', 'W-OLD-INFO')",
    )
    .await;

    let now = Utc::now();
    let open = crate::di::queries::list_intervention_requests(&db, di_scope(true, None))
        .await
        .expect("open list");
    let open_count = crate::analytics::as_of::di_open_at(&db, now, None, None)
        .await
        .expect("open count");
    assert_eq!(open.total, open_count);
    assert_eq!(open.total, 3, "W-NEW, W-OLD, W-OLD-INFO");

    let waiting = crate::di::queries::list_intervention_requests(&db, di_scope(false, Some(7)))
        .await
        .expect("waiting list");
    let waiting_count = crate::analytics::as_of::di_open_at(&db, now, Some(7), None)
        .await
        .expect("waiting count");
    assert_eq!(waiting.total, waiting_count);
    let mut codes: Vec<&str> = waiting.items.iter().map(|x| x.code.as_str()).collect();
    codes.sort_unstable();
    assert_eq!(codes, ["W-OLD", "W-OLD-INFO"]);
}

/// A legacy terminal request (`rejected`, … folded into `closed` by migration 139, still found in
/// restored backups) is closed everywhere: not open on the home page, not in the open list, not in
/// "my requests", not on the asset card, and the as-of count agrees.
#[tokio::test]
async fn a_legacy_rejected_request_is_never_open() {
    let db = setup().await;
    let me = admin_id(&db).await;
    request(&db, "LG-OPEN", "submitted", me, false).await;
    for (code, status) in [
        ("LG-REJ", "rejected"),
        ("LG-CONV", "converted_to_work_order"),
        ("LG-NONEX", "closed_as_non_executable"),
        ("LG-ARCH", "archived"),
    ] {
        request(&db, code, status, me, false).await;
    }
    let now = Utc::now();
    let as_of = crate::analytics::as_of::di_open_at(&db, now, None, None)
        .await
        .expect("as of");
    let list = crate::di::queries::list_intervention_requests(&db, di_scope(true, None))
        .await
        .expect("open list");
    let module = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM intervention_requests ir WHERE {}",
            crate::di::stats::OPEN_SQL
        ),
    )
    .await;
    assert_eq!((as_of, list.total, module), (1, 1, 1), "only LG-OPEN is open");
    assert_eq!(list.items[0].code, "LG-OPEN");

    let mine = my_requests(&db, me).await.expect("my requests");
    assert_eq!(mine.open, 1);

    // The dashboard summary report counts the same open requests as the home page.
    let report = crate::reports::queries::fetch_dashboard_summary(&db)
        .await
        .expect("report summary");
    assert_eq!(report.open_di, as_of, "report open DI == home open DI");

    let rejected_asset = scalar(
        &db,
        "SELECT equipment_id FROM intervention_requests WHERE code = 'LG-REJ'",
    )
    .await;
    assert_eq!(
        crate::assets::bindings::count_open_di(&db, rejected_asset)
            .await
            .expect("asset open di"),
        0
    );
    // Waiting since long ago: still never open.
    exec(
        &db,
        "UPDATE intervention_requests SET submitted_at = datetime('now', '-30 days')",
    )
    .await;
    let waiting = crate::di::queries::list_intervention_requests(&db, di_scope(false, Some(7)))
        .await
        .expect("waiting list");
    assert_eq!(waiting.total, 1);
}

#[test]
fn one_terminal_list_for_the_open_rule_and_the_as_of_counts() {
    let terminal = crate::di::stats::TERMINAL_STATUSES_SQL;
    assert!(crate::di::stats::OPEN_SQL.contains(terminal));
    for legacy in [
        "'rejected'",
        "'converted_to_work_order'",
        "'closed_as_non_executable'",
        "'archived'",
    ] {
        assert!(terminal.contains(legacy), "{legacy}");
    }
}
