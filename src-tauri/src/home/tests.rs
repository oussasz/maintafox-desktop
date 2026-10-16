//! Home page panels on a real migrated + seeded in-memory database: every figure follows the
//! owning module's rule, sections follow the person's access, nothing fails silently to 0.

use chrono::{NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::my_work::{build_my_work, my_pm, my_requests, my_rounds, my_work_orders, MyWorkAccess};
use super::planning::{
    applicable_work_week, week_load, work_week_bounds, WorkWeekBasis, WORK_WEEK_NOT_CONFIGURED, WORK_WEEK_NO_REST_DAY,
    WORK_WEEK_SITE_AMBIGUOUS, WORK_WEEK_SPLIT_REST,
};
use super::supervision::{assets, build_supervision, permit_list, work_orders, SupervisionAccess};
use super::{checked_period, period_first_day, period_window, urgency_rank_sql};
use crate::commands::dashboard::{build_workload_chart, WorkloadSeries};
use crate::planning::domain::TeamCapacityLoad;

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
fn the_work_week_starts_the_day_after_the_rest_days() {
    // Thursday 15 October 2026. Rest Friday–Saturday (5, 6): the week runs Sunday 11 → Saturday 17.
    let thursday = date(2026, 10, 15);
    assert_eq!(
        work_week_bounds(thursday, &[5, 6]),
        Ok((date(2026, 10, 11), date(2026, 10, 17)))
    );
    // On the rest days themselves the week is still the one they close.
    assert_eq!(
        work_week_bounds(date(2026, 10, 17), &[5, 6]),
        Ok((date(2026, 10, 11), date(2026, 10, 17)))
    );
    // The Sunday after starts the next week.
    assert_eq!(
        work_week_bounds(date(2026, 10, 18), &[5, 6]),
        Ok((date(2026, 10, 18), date(2026, 10, 24)))
    );
    // Rest Saturday–Sunday: Monday 12 → Sunday 18.
    assert_eq!(
        work_week_bounds(thursday, &[6, 7]),
        Ok((date(2026, 10, 12), date(2026, 10, 18)))
    );
    // Rest Sunday only (wraps): Monday 12 → Sunday 18.
    assert_eq!(
        work_week_bounds(thursday, &[7]),
        Ok((date(2026, 10, 12), date(2026, 10, 18)))
    );
    assert_eq!(work_week_bounds(thursday, &[]), Err(WORK_WEEK_NO_REST_DAY));
    assert_eq!(work_week_bounds(thursday, &[3, 6]), Err(WORK_WEEK_SPLIT_REST));
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

async fn set_week(db: &DatabaseConnection, sid: &str, entity: Option<i64>, rest: &str) {
    let entity = entity.map_or("NULL".to_string(), |e| e.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO site_work_weeks (entity_sync_id, entity_id, effective_from, weekend_days, created_at, \
             updated_at) VALUES ('{sid}', {entity}, '2020-01-01', '{rest}', datetime('now'), datetime('now'))"
        ),
    )
    .await;
}

#[tokio::test]
async fn the_work_week_comes_from_the_site_calendar_never_a_default() {
    let db = setup().await;
    let me = admin_id(&db).await;
    let today = date(2026, 10, 15);
    exec(&db, "DELETE FROM site_work_weeks").await;
    assert_eq!(
        applicable_work_week(&db, me, today).await.expect("week"),
        Err(WORK_WEEK_NOT_CONFIGURED)
    );

    set_week(&db, "hm-week-all", None, "5,6").await;
    assert_eq!(
        applicable_work_week(&db, me, today).await.expect("week"),
        Ok(WorkWeekBasis {
            rest_days: vec![5, 6],
            scope: "all_sites".into(),
            site_name: None
        })
    );

    // Site A rests Saturday–Sunday: sites differ, and the admin's login has no personnel site.
    set_week(&db, "hm-week-a", Some(1), "6,7").await;
    exec(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = NULL WHERE id = {me}"),
    )
    .await;
    assert_eq!(
        applicable_work_week(&db, me, today).await.expect("week"),
        Err(WORK_WEEK_SITE_AMBIGUOUS)
    );
}

fn day(team_id: i64, available: f64, overtime: f64, committed: f64) -> TeamCapacityLoad {
    TeamCapacityLoad {
        team_id,
        work_date: "2026-10-12".into(),
        available_hours: available,
        overtime_hours: overtime,
        committed_hours: committed,
        utilization_ratio: None,
        basis: "roster".into(),
        work_windows: Vec::new(),
        unwindowed_hours: 0.0,
    }
}

#[test]
fn week_load_is_pooled_and_counts_overloaded_crew_days() {
    let mon = NaiveDate::from_ymd_opt(2026, 10, 12).expect("date");
    let sun = NaiveDate::from_ymd_opt(2026, 10, 18).expect("date");
    // Team 1: 8 h + 2 h overtime, 12 h committed (overloaded); team 2: 16 h, 4 h; team 2 rest day
    // with 3 h committed (overloaded: no hours at all).
    let rows = [day(1, 8.0, 2.0, 12.0), day(2, 16.0, 0.0, 4.0), day(2, 0.0, 0.0, 3.0)];
    let basis = WorkWeekBasis {
        rest_days: vec![6, 7],
        scope: "all_sites".into(),
        site_name: None,
    };
    let w = week_load(&rows, mon, sun, basis.clone());
    assert_eq!(
        (w.week_start.as_str(), w.week_end.as_str()),
        ("2026-10-12", "2026-10-18")
    );
    assert_eq!(w.teams, 2);
    assert!((w.capacity_hours - 26.0).abs() < 1e-9);
    assert!((w.committed_hours - 19.0).abs() < 1e-9);
    let pct = w.load_pct.expect("capacity");
    assert!(
        (pct - 19.0 / 26.0 * 100.0).abs() < 1e-9,
        "pooled, not a mean of daily ratios: {pct}"
    );
    assert_eq!(w.overloaded_team_days, 2);

    let none = week_load(&[day(1, 0.0, 0.0, 0.0)], mon, sun, basis);
    assert_eq!(none.load_pct, None, "no hours: no percentage, never 0 % or 100 %");
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
