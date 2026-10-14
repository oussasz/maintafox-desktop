//! Analytics: dashboard layout validation / merge / per-user storage, the Overview KPI cells and
//! the executive dashboard projection, on a real migrated + seeded in-memory database.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Value};

use crate::analytics::layout::{
    get_layout, merge_overview_kpis_into_layout, save_layout, validate_analytics_layout_json,
    DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON,
};
use crate::analytics::overview_kpis::{build_overview_kpis, OverviewKpiCell, OverviewKpisPayload};
use crate::analytics::projection::{build_executive_dashboard, AnalyticsExecutiveFilter};
use crate::errors::AppError;
use crate::kpi_definitions;

// ── Fixture helpers ─────────────────────────────────────────────────────────────────────────

async fn setup() -> DatabaseConnection {
    crate::db::test_db::seeded().await
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

async fn text(db: &DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap_or_else(|| panic!("{sql}: no row"))
        .try_get_by_index::<String>(0)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    scalar(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

async fn insert_user(db: &DatabaseConnection, username: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, \
             is_admin, force_password_change, created_at, updated_at) \
             VALUES ('an-user-{username}', '{username}', '{username}', 'local', 1, 0, 0, \
             strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM user_accounts WHERE username = '{username}'"),
    )
    .await
}

/// Ids of the seeded dataset.
struct Data {
    e1: i64,
    e2: i64,
}

/// Two sites (org node 1 and 2), two live assets (E1 on site 1, E2 on site 2) and one deleted one.
///
/// Intervention requests (DI):
/// - DI1 submitted, site 1, now; DI2 submitted, site 1, 10 days ago; DI5 submitted, site 2, 40 days ago
/// - DI3 rejected and DI4 converted_to_work_order, site 1, now (not open)
///
/// Work orders:
/// - WO1 in_progress, E1, site 1, 4 h planned, planned_end 3 days ago, created/updated now
/// - WO2 draft, E2, site 2, 6 h planned, planned_end in 5 days, created/updated 10 days ago
/// - WO3 closed, E1, site 1, total cost 100, closed 5 days ago
/// - WO4 closed, E1, site 1, total cost 40, closed 45 days ago
/// - WO5 cancelled, E1, site 1, planned_end 20 days ago
///
/// PM occurrences on a plan scoped to E1: O1 due 3 days ago (open), O2 due in 3 days (open),
/// O3 due 5 days ago (completed).
///
/// Reliability snapshots: E1 latest (2 days ago: 1000 h, 4 failures, 4 repairs / 8 h, 20 h down,
/// MTBF 250), E1 older (10 days ago: 99 failures, MTBF 9), E2 (3 days ago: 500 h, 1 failure,
/// 1 repair / 2 h, 5 h down, DQ 0.9, MTBF 500).
async fn seed_dataset(db: &DatabaseConnection) -> Data {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'an-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'an-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'an-node-1', 'SITE-A', 'Site A', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (2, 'an-node-2', 'SITE-B', 'Site B', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('an-eq-1', 'PMP-01', 'Pompe 1', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('an-eq-2', 'CMP-02', 'Compresseur 2', 'active_in_service', 2, \
         datetime('now'), datetime('now'))",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at, deleted_at) VALUES ('an-eq-3', 'OLD-03', 'Retired', 'decommissioned', 1, \
         datetime('now'), datetime('now'), datetime('now'))",
    ] {
        exec(db, sql).await;
    }
    let e1 = scalar(db, "SELECT id FROM equipment WHERE asset_id_code = 'PMP-01'").await;
    let e2 = scalar(db, "SELECT id FROM equipment WHERE asset_id_code = 'CMP-02'").await;
    let admin = admin_id(db).await;

    for (code, eq, node, status, created) in [
        ("DI-1", e1, 1, "submitted", "datetime('now')"),
        ("DI-2", e1, 1, "submitted", "datetime('now', '-10 days')"),
        ("DI-3", e1, 1, "rejected", "datetime('now')"),
        ("DI-4", e1, 1, "converted_to_work_order", "datetime('now')"),
        ("DI-5", e2, 2, "submitted", "datetime('now', '-40 days')"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO intervention_requests \
                 (code, equipment_id, org_node_id, status, title, description, origin_type, impact_level, \
                  reported_urgency, submitted_at, submitter_id, created_at, updated_at) \
                 VALUES ('{code}', {eq}, {node}, '{status}', '{code}', 'desc', 'operator', 'unknown', 'medium', \
                  {created}, {admin}, {created}, {created})"
            ),
        )
        .await;
    }

    let type_id = scalar(db, "SELECT id FROM work_order_types ORDER BY id LIMIT 1").await;
    for (code, status, eq, node, hours, planned_end, closed_at, cost, created) in [
        (
            "WO-1",
            "in_progress",
            e1,
            1,
            "4.0",
            "strftime('%Y-%m-%dT%H:%M:%SZ','now','-3 days')",
            "NULL",
            "0",
            "datetime('now')",
        ),
        (
            "WO-2",
            "draft",
            e2,
            2,
            "6.0",
            "strftime('%Y-%m-%dT%H:%M:%SZ','now','+5 days')",
            "NULL",
            "0",
            "datetime('now', '-10 days')",
        ),
        (
            "WO-3",
            "closed",
            e1,
            1,
            "NULL",
            "strftime('%Y-%m-%dT%H:%M:%SZ','now','-20 days')",
            "datetime('now', '-5 days')",
            "100.0",
            "datetime('now', '-20 days')",
        ),
        (
            "WO-4",
            "closed",
            e1,
            1,
            "NULL",
            "NULL",
            "datetime('now', '-45 days')",
            "40.0",
            "datetime('now', '-50 days')",
        ),
        (
            "WO-5",
            "cancelled",
            e1,
            1,
            "3.0",
            "strftime('%Y-%m-%dT%H:%M:%SZ','now','-20 days')",
            "NULL",
            "0",
            "datetime('now', '-25 days')",
        ),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO work_orders (code, type_id, status_id, equipment_id, entity_id, title, \
                 expected_duration_hours, planned_end, closed_at, total_cost, row_version, created_at, updated_at) \
                 VALUES ('{code}', {type_id}, (SELECT id FROM work_order_statuses WHERE code = '{status}'), \
                 {eq}, {node}, '{code}', {hours}, {planned_end}, {closed_at}, {cost}, 1, {created}, {created})"
            ),
        )
        .await;
    }

    let plan = crate::pm::queries::create_pm_plan(
        db,
        crate::pm::domain::CreatePmPlanInput {
            code: "PM-AN".into(),
            title: "Pump inspection".into(),
            description: None,
            asset_scope_type: "equipment".into(),
            asset_scope_id: Some(e1),
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
        db,
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
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("pm version");
    for (due, status) in [
        ("datetime('now', '-3 days')", "forecasted"),
        ("datetime('now', '+3 days')", "forecasted"),
        ("datetime('now', '-5 days')", "completed"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO pm_occurrences (pm_plan_id, plan_version_id, due_basis, due_at, status) \
                 VALUES ({p}, (SELECT id FROM pm_plan_versions WHERE pm_plan_id = {p} ORDER BY id LIMIT 1), \
                 'calendar', {due}, '{status}')",
                p = plan.id
            ),
        )
        .await;
    }

    for (sid, eq, end, events, exposure, reps, active, down, dq, mtbf) in [
        ("an-snap-1", e1, "-2 days", 4, 1000.0, 4, 8.0, 20.0, 1.0, 250.0),
        ("an-snap-2", e1, "-10 days", 99, 900.0, 1, 1.0, 1.0, 1.0, 9.0),
        ("an-snap-3", e2, "-3 days", 1, 500.0, 1, 2.0, 5.0, 0.9, 500.0),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO reliability_kpi_snapshots (entity_sync_id, equipment_id, period_start, period_end, \
                 mtbf, availability, event_count, data_quality_score, exposure_hours, repair_event_count, \
                 active_repair_hours_total, downtime_hours_total) \
                 VALUES ('{sid}', {eq}, datetime('now', '-400 days'), datetime('now', '{end}'), {mtbf}, 0.95, \
                 {events}, {dq}, {exposure}, {reps}, {active}, {down})"
            ),
        )
        .await;
    }
    Data { e1, e2 }
}

fn cell<'a>(p: &'a OverviewKpisPayload, key: &str) -> &'a OverviewKpiCell {
    p.cells
        .iter()
        .find(|c| c.semantic_key == key)
        .unwrap_or_else(|| panic!("cell {key} missing"))
}

fn filter(horizon_days: i64, org_node_id: Option<i64>) -> AnalyticsExecutiveFilter {
    AnalyticsExecutiveFilter {
        horizon_days,
        org_node_id,
    }
}

fn layout_with_widgets(n: usize) -> String {
    let widgets: Vec<Value> = (0..n)
        .map(|i| json!({"id": format!("w{i}"), "order": i, "visible": true}))
        .collect();
    json!({"version": 1, "widgets": widgets}).to_string()
}

fn validation_message(r: Result<(), AppError>) -> String {
    match r {
        Err(AppError::ValidationFailed(m)) => m.join(" | "),
        other => panic!("expected ValidationFailed, got {other:?}"),
    }
}

// ── Layout validation ───────────────────────────────────────────────────────────────────────

#[test]
fn default_layout_is_valid_and_lists_six_widgets() {
    validate_analytics_layout_json(DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON).expect("default valid");
    let v: Value = serde_json::from_str(DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON).expect("json");
    let ids: Vec<&str> = v["widgets"]
        .as_array()
        .expect("widgets")
        .iter()
        .map(|w| w["id"].as_str().expect("id"))
        .collect();
    assert_eq!(
        ids,
        [
            "ops_kpis",
            "pm_due",
            "reliability_rams",
            "maintenance_cost",
            "workload",
            "di_status"
        ]
    );
}

#[test]
fn layout_validation_accepts_bounds_and_known_accents() {
    validate_analytics_layout_json(&layout_with_widgets(1)).expect("1 widget");
    validate_analytics_layout_json(&layout_with_widgets(32)).expect("32 widgets");
    let id64 = "x".repeat(64);
    validate_analytics_layout_json(
        &json!({"version": 1, "widgets": [{"id": id64, "order": -3, "visible": false}]}).to_string(),
    )
    .expect("64-char id, negative order");
    let kpis: Vec<Value> = ["default", "brand", "amber", "danger", "teal"]
        .iter()
        .enumerate()
        .map(|(i, a)| json!({"id": format!("k{i}"), "order": i, "visible": true, "accent": a}))
        .chain(std::iter::once(
            json!({"id": "k-null", "order": 9, "visible": false, "accent": null}),
        ))
        .collect();
    validate_analytics_layout_json(
        &json!({"version": 1, "widgets": [{"id": "a", "order": 0, "visible": true}], "overview_kpis": kpis})
            .to_string(),
    )
    .expect("known accents and null accent");
    validate_analytics_layout_json(
        &json!({"version": 1, "widgets": [{"id": "a", "order": 0, "visible": true}], "overview_kpis": []}).to_string(),
    )
    .expect("empty overview_kpis is allowed");
}

#[test]
fn layout_validation_rejects_each_malformed_shape() {
    let w = json!([{"id": "a", "order": 0, "visible": true}]);
    let cases: Vec<(String, &str)> = vec![
        ("not json".into(), "layout_json:"),
        ("[1,2]".into(), "must be an object"),
        (json!({"version": 2, "widgets": w}).to_string(), "version must be 1"),
        (json!({"widgets": w}).to_string(), "version must be 1"),
        (json!({"version": 1}).to_string(), "widgets required"),
        (json!({"version": 1, "widgets": {}}).to_string(), "widgets required"),
        (json!({"version": 1, "widgets": []}).to_string(), "widgets length invalid"),
        (layout_with_widgets(33), "widgets length invalid"),
        (json!({"version": 1, "widgets": ["a"]}).to_string(), "each widget must be an object"),
        (
            json!({"version": 1, "widgets": [{"order": 0, "visible": true}]}).to_string(),
            "widget.id invalid",
        ),
        (
            json!({"version": 1, "widgets": [{"id": "", "order": 0, "visible": true}]}).to_string(),
            "widget.id invalid",
        ),
        (
            json!({"version": 1, "widgets": [{"id": "x".repeat(65), "order": 0, "visible": true}]}).to_string(),
            "widget.id invalid",
        ),
        (
            json!({"version": 1, "widgets": [{"id": "a", "order": 1.5, "visible": true}]}).to_string(),
            "widget.order invalid",
        ),
        (
            json!({"version": 1, "widgets": [{"id": "a", "order": 0, "visible": "yes"}]}).to_string(),
            "widget.visible invalid",
        ),
        (
            json!({"version": 1, "widgets": w, "overview_kpis": [{"id": "k", "order": 0, "visible": true, "accent": "pink"}]})
                .to_string(),
            "overview_kpi.accent unknown",
        ),
        (
            json!({"version": 1, "widgets": w, "overview_kpis": [{"id": "k", "order": 0, "visible": true, "accent": 3}]})
                .to_string(),
            "overview_kpi.accent invalid",
        ),
        (
            json!({"version": 1, "widgets": w, "overview_kpis": [{"id": "k", "order": 0}]}).to_string(),
            "overview_kpi.visible invalid",
        ),
        (
            json!({"version": 1, "widgets": w, "overview_kpis": [7]}).to_string(),
            "each overview_kpi must be an object",
        ),
    ];
    for (raw, expected) in cases {
        let msg = validation_message(validate_analytics_layout_json(&raw));
        assert!(msg.contains(expected), "{raw}: expected '{expected}' in '{msg}'");
    }

    let too_many_kpis: Vec<Value> = (0..33)
        .map(|i| json!({"id": format!("k{i}"), "order": i, "visible": true}))
        .collect();
    let msg = validation_message(validate_analytics_layout_json(
        &json!({"version": 1, "widgets": w, "overview_kpis": too_many_kpis}).to_string(),
    ));
    assert!(msg.contains("overview_kpis length invalid"), "{msg}");

    let huge = json!({"version": 1, "widgets": w, "pad": "p".repeat(48_000)}).to_string();
    let msg = validation_message(validate_analytics_layout_json(&huge));
    assert!(msg.contains("too large"), "{msg}");
}

// ── Merge of overview KPIs ──────────────────────────────────────────────────────────────────

#[test]
fn merge_overview_kpis_keeps_widgets_and_replaces_previous_kpis() {
    let custom = json!({"version": 1, "widgets": [{"id": "only", "order": 0, "visible": false}],
                        "overview_kpis": [{"id": "old", "order": 0, "visible": true}]})
    .to_string();
    let kpis = json!([{"id": "DI_OPEN", "order": 0, "visible": true, "accent": "danger"},
                      {"id": "MTBF", "order": 1, "visible": false}]);
    let out = merge_overview_kpis_into_layout(&custom, kpis.clone()).expect("merge");
    let v: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["version"], json!(1));
    assert_eq!(v["widgets"], json!([{"id": "only", "order": 0, "visible": false}]));
    assert_eq!(
        v["overview_kpis"], kpis,
        "previous overview_kpis are replaced, not appended"
    );
    validate_analytics_layout_json(&out).expect("merged layout is valid");
}

#[test]
fn merge_overview_kpis_fills_missing_version_and_widgets_from_default() {
    let default_widgets =
        serde_json::from_str::<Value>(DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON).expect("json")["widgets"].clone();
    let kpis = json!([{"id": "TOTAL_COST", "order": 0, "visible": true}]);

    for existing in ["{}", "this is not json"] {
        let out = merge_overview_kpis_into_layout(existing, kpis.clone()).expect("merge");
        let v: Value = serde_json::from_str(&out).expect("json");
        assert_eq!(v["version"], json!(1), "{existing}");
        assert_eq!(v["widgets"], default_widgets, "{existing}");
        assert_eq!(v["overview_kpis"], kpis, "{existing}");
    }
}

#[test]
fn merge_overview_kpis_rejects_invalid_kpis_and_non_object_layout() {
    let err = merge_overview_kpis_into_layout(
        DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON,
        json!([{"id": "DI_OPEN", "order": 0, "visible": true, "accent": "neon"}]),
    );
    assert!(
        matches!(err, Err(AppError::ValidationFailed(ref m)) if m[0].contains("accent unknown")),
        "{err:?}"
    );
    let err = merge_overview_kpis_into_layout("[]", json!([]));
    assert!(
        matches!(err, Err(AppError::ValidationFailed(ref m)) if m[0].contains("must be an object")),
        "{err:?}"
    );
}

// ── Per-user layout storage ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_layout_defaults_then_returns_saved_layout_per_user() {
    let db = setup().await;
    let admin = admin_id(&db).await;
    let other = insert_user(&db, "an_planner").await;

    assert_eq!(
        get_layout(&db, admin).await.expect("get"),
        DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON
    );

    let first = layout_with_widgets(2);
    save_layout(&db, admin, &first).await.expect("save");
    assert_eq!(get_layout(&db, admin).await.expect("get"), first);
    assert_eq!(
        get_layout(&db, other).await.expect("get other"),
        DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON,
        "another user's layout is independent"
    );

    let second = layout_with_widgets(3);
    save_layout(&db, admin, &second).await.expect("save again");
    assert_eq!(get_layout(&db, admin).await.expect("get"), second);
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) FROM user_analytics_dashboard_layouts WHERE user_id = {admin}")
        )
        .await,
        1,
        "save upserts the user's single row"
    );
}

#[tokio::test]
async fn save_layout_rejects_invalid_json_and_keeps_stored_layout() {
    let db = setup().await;
    let admin = admin_id(&db).await;
    let good = layout_with_widgets(4);
    save_layout(&db, admin, &good).await.expect("save");

    let err = save_layout(&db, admin, r#"{"version":1,"widgets":[]}"#).await;
    assert!(matches!(err, Err(AppError::ValidationFailed(_))), "{err:?}");
    assert_eq!(
        text(
            &db,
            &format!("SELECT layout_json FROM user_analytics_dashboard_layouts WHERE user_id = {admin}")
        )
        .await,
        good
    );
}

#[tokio::test]
async fn get_layout_falls_back_to_default_when_stored_row_is_corrupt() {
    let db = setup().await;
    let admin = admin_id(&db).await;
    exec(
        &db,
        &format!(
            "INSERT INTO user_analytics_dashboard_layouts (user_id, layout_json, updated_at) \
             VALUES ({admin}, '{{\"version\":7}}', datetime('now'))"
        ),
    )
    .await;
    assert_eq!(
        get_layout(&db, admin).await.expect("get"),
        DEFAULT_ANALYTICS_DASHBOARD_LAYOUT_JSON
    );
}

// ── Overview KPIs ───────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn overview_kpis_count_the_seeded_dataset() {
    let db = setup().await;
    seed_dataset(&db).await;
    let p = build_overview_kpis(&db, true, true, true, true, true, true, true)
        .await
        .expect("overview");
    assert_eq!(p.horizon_days, 30);
    use kpi_definitions::overview as k;
    let keys: Vec<&str> = p.cells.iter().map(|c| c.semantic_key.as_str()).collect();
    assert_eq!(
        keys,
        [
            k::DI_OPEN,
            k::OT_IN_PROGRESS,
            k::OT_OVERDUE,
            k::PM_COMPLIANCE,
            k::PM_DUE_TODAY,
            k::MAINT_BACKLOG,
            k::MTBF,
            k::MTTR,
            k::AVAILABILITY,
            k::DATA_QUALITY,
            k::CRITICAL_DOWN,
            k::STOCK_OUT,
            k::REORDER_ALERT,
            k::TOTAL_COST,
        ]
    );
    assert!(p.cells.iter().all(|c| c.available), "every permission granted");

    // DI: DI1, DI2, DI5 open (DI3 rejected, DI4 converted); 30 days ago only DI5 (−40 d) existed.
    let di = cell(&p, k::DI_OPEN);
    assert_eq!((di.value, di.previous_value), (3.0, 1.0));
    assert!(di.baseline_available);
    assert_eq!(di.catalog_id, kpi_definitions::catalog::DI_OPEN);
    assert_eq!(di.quality_badge, None);
    assert_eq!(di.value_kind, "count");
    assert_eq!(di.trend_sense, "up-bad");
    // Created today: DI1, DI3, DI4; 10 days ago: DI2 (DI5 is outside 30 days).
    assert_eq!(di.series, Some(vec![1.0, 3.0]));

    let ip = cell(&p, k::OT_IN_PROGRESS);
    assert_eq!((ip.value, ip.previous_value), (1.0, 0.0));
    assert_eq!(
        ip.quality_badge.as_deref(),
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE)
    );
    assert_eq!(ip.series, Some(vec![1.0]));

    // WO1 is past its planned end; WO3 (closed) and WO5 (cancelled) do not count.
    let od = cell(&p, k::OT_OVERDUE);
    assert_eq!((od.value, od.previous_value), (1.0, 0.0));

    // PM compliance: O1 (due −3 d, not done) and O3 (due −5 d, completed but with no execution
    // and a completion stamp of today, after its due day) → 0 of 2 on time; nothing due 30–60 days
    // ago, so no baseline.
    let pm = cell(&p, k::PM_COMPLIANCE);
    assert_eq!((pm.value, pm.value_kind.as_str()), (0.0, "percent"));
    assert!(pm.available);
    assert!(!pm.baseline_available);
    let today = cell(&p, k::PM_DUE_TODAY);
    assert_eq!((today.value, today.previous_value), (0.0, 0.0));

    // Backlog: open WO1 (4 h) + WO2 (6 h); 30 days ago only WO4 existed and it was closed at −45 d.
    let bl = cell(&p, k::MAINT_BACKLOG);
    assert_eq!((bl.value, bl.previous_value), (10.0, 0.0));
    assert_eq!(bl.value_kind, "hours");
    assert_eq!(bl.series, Some(vec![6.0, 4.0]));

    // Fleet RAMS over the latest snapshot per asset: T = 1000 + 500, F = 4 + 1.
    let mtbf = cell(&p, k::MTBF);
    assert!(close(mtbf.value, 300.0), "{}", mtbf.value);
    assert_eq!(mtbf.previous_value, 0.0, "no snapshot 30-60 days ago");
    assert_eq!(
        mtbf.quality_badge.as_deref(),
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE)
    );
    assert_eq!(
        mtbf.series,
        Some(vec![9.0, 500.0, 250.0]),
        "daily mean MTBF by snapshot date"
    );
    assert!(close(cell(&p, k::MTTR).value, 2.0));
    let av = cell(&p, k::AVAILABILITY);
    assert!(close(av.value, 1500.0 / 1525.0 * 100.0), "{}", av.value);
    assert_eq!(av.value_kind, "percent");
    assert!(close(cell(&p, k::DATA_QUALITY).value, 95.0));

    assert_eq!(cell(&p, k::CRITICAL_DOWN).value, 0.0);
    for key in [k::CRITICAL_DOWN, k::STOCK_OUT, k::REORDER_ALERT] {
        assert!(!cell(&p, key).baseline_available, "{key}: no history to rebuild");
    }
    assert_eq!(cell(&p, k::STOCK_OUT).value, 0.0);
    assert_eq!(cell(&p, k::REORDER_ALERT).value, 0.0);

    // Cost: WO3 closed 5 days ago (100); WO4 closed 45 days ago (40) is the previous window.
    let cost = cell(&p, k::TOTAL_COST);
    assert_eq!((cost.value, cost.previous_value), (100.0, 40.0));
    assert_eq!(cost.value_kind, "currency");
    assert_eq!(cost.series, Some(vec![100.0]));
}

#[tokio::test]
async fn overview_kpis_without_permissions_are_unavailable_and_empty() {
    let db = setup().await;
    seed_dataset(&db).await;
    let p = build_overview_kpis(&db, false, false, false, false, false, false, false)
        .await
        .expect("overview");
    assert_eq!(p.cells.len(), 14);
    for c in &p.cells {
        assert!(!c.available, "{}", c.semantic_key);
        assert_eq!((c.value, c.previous_value), (0.0, 0.0), "{}", c.semantic_key);
        assert_eq!(c.quality_badge, None, "{}", c.semantic_key);
        assert_eq!(c.series, None, "{}", c.semantic_key);
    }
}

#[tokio::test]
async fn overview_reliability_cells_unavailable_without_snapshots() {
    let db = setup().await;
    let p = build_overview_kpis(&db, false, false, false, false, true, false, false)
        .await
        .expect("overview");
    for key in [
        kpi_definitions::overview::MTBF,
        kpi_definitions::overview::MTTR,
        kpi_definitions::overview::AVAILABILITY,
        kpi_definitions::overview::DATA_QUALITY,
    ] {
        let c = cell(&p, key);
        assert!(!c.available, "{key} has no snapshot to compute from");
        assert_eq!(c.value, 0.0);
    }
}

// ── Executive dashboard projection ──────────────────────────────────────────────────────────

#[tokio::test]
async fn executive_dashboard_fleet_wide_values() {
    let db = setup().await;
    let d = seed_dataset(&db).await;
    let x = build_executive_dashboard(&db, &filter(30, None), true, true, true, true, true)
        .await
        .expect("dashboard");
    assert_eq!((x.horizon_days, x.org_node_id), (30, None));

    assert_eq!(
        (x.open_dis.value, x.open_dis.previous_value),
        (3.0, 1.0),
        "DI5 is older than 30 days"
    );
    assert_eq!(x.open_dis.quality_badge, None);
    assert_eq!((x.open_wos.value, x.open_wos.previous_value), (2.0, 0.0));
    assert_eq!(
        x.open_wos.quality_badge.as_deref(),
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE)
    );
    assert_eq!(x.total_assets.value, 2.0, "deleted asset excluded");
    // DI2 and DI5 waiting more than 7 days + PM O1 overdue.
    assert_eq!(x.overdue_items.value, 3.0);

    assert_eq!(x.pm_overdue.value, 1.0);
    assert_eq!(x.pm_due_today.value, 0.0);
    assert_eq!(x.pm_due_next_7d.value, 1.0);
    assert_eq!(
        x.pm_overdue.catalog_id.as_deref(),
        Some(kpi_definitions::catalog::PM_ADHERENCE)
    );

    assert!(close(x.avg_mtbf_hours.value, 300.0));
    assert!(close(x.avg_mttr_hours.value, 2.0));
    assert!(close(x.avg_availability.value, 1500.0 / 1525.0));
    assert!(close(x.avg_data_quality.value, 0.95));
    assert!(x.avg_mtbf_hours.available);
    assert_eq!(x.reliability_snapshot_count, 2, "one (latest) snapshot per asset");
    assert_eq!(x.reliability_total_events, 5);

    assert_eq!(
        (x.maintenance_cost.value, x.maintenance_cost.previous_value),
        (100.0, 40.0)
    );

    // Workload axis: one entry per day of the horizon, ending today.
    assert_eq!(x.workload_days.len(), 30);
    let today = chrono::Utc::now().date_naive();
    assert_eq!(
        x.workload_days.last().expect("day").date,
        today.format("%Y-%m-%d").to_string()
    );
    assert_eq!(
        x.workload_days.first().expect("day").date,
        (today - chrono::Duration::days(29)).format("%Y-%m-%d").to_string()
    );
    assert_eq!(x.workload_days.last().expect("day").di_created, 3);
    assert_eq!(x.workload_days.iter().map(|w| w.di_created).sum::<i64>(), 4);
    assert_eq!(x.workload_days.iter().map(|w| w.wo_completed).sum::<i64>(), 1);
    assert_eq!(x.workload_days[30 - 1 - 5].wo_completed, 1, "WO3 closed 5 days ago");
    assert_eq!(
        x.workload_days.iter().map(|w| w.pm_due).sum::<i64>(),
        2,
        "O1 and O3 (the future O2 is off the axis)"
    );
    assert_eq!(x.workload_quality_badge, None);

    assert_eq!(x.di_status_segments[0].status, "submitted");
    assert_eq!(x.di_status_segments[0].count, 3);
    let mut rest: Vec<(String, i64)> = x.di_status_segments[1..]
        .iter()
        .map(|s| (s.status.clone(), s.count))
        .collect();
    rest.sort();
    assert_eq!(
        rest,
        [("converted_to_work_order".to_string(), 1), ("rejected".to_string(), 1)]
    );

    // RAMS series: snapshot dates 10, 3 and 2 days ago plus the cost of WO3 (5 days ago).
    assert_eq!(x.reliability_series.len(), 4);
    let dates: Vec<&str> = x.reliability_series.iter().map(|p| p.date.as_str()).collect();
    let mut sorted = dates.clone();
    sorted.sort_unstable();
    assert_eq!(dates, sorted, "series is in date order");
    let cost_day = (today - chrono::Duration::days(5)).format("%Y-%m-%d").to_string();
    let cost_point = x
        .reliability_series
        .iter()
        .find(|p| p.date == cost_day)
        .expect("cost point");
    assert_eq!(cost_point.maintenance_cost, Some(100.0));
    assert_eq!(cost_point.avg_mtbf, None);
    let last = x.reliability_series.last().expect("point");
    assert_eq!(last.avg_mtbf, Some(1000.0 / 4.0), "E1 alone on its latest snapshot day");

    // Bad actors: latest snapshot per asset, most events first, with closed-WO cost in horizon.
    assert_eq!(x.bad_actor_assets.len(), 2);
    let top = &x.bad_actor_assets[0];
    assert_eq!((top.equipment_id, top.asset_code.as_str()), (d.e1, "PMP-01"));
    assert_eq!(top.event_count, 4, "older snapshot with 99 events is superseded");
    assert_eq!(top.mtbf_hours, Some(250.0));
    assert_eq!(top.maintenance_cost, 100.0);
    assert!(top.cost_available);
    assert_eq!(
        (
            x.bad_actor_assets[1].equipment_id,
            x.bad_actor_assets[1].maintenance_cost
        ),
        (d.e2, 0.0)
    );
    // SMRP set over the same 30 days: WO3 (closed −5 d, no labour, no hours) is the only
    // completed work order → counted by work orders.
    let smrp = x.smrp.expect("smrp");
    let mix = smrp.work_mix.expect("work mix");
    assert_eq!((mix.basis.as_str(), mix.work_orders), ("count", 1));
    let backlog = smrp.backlog.expect("backlog");
    assert!(close(backlog.backlog_hours, 10.0), "WO1 4 h + WO2 6 h");
    assert_eq!(backlog.weeks, None, "no crew schedule, no capacity");
    assert_eq!(smrp.cost_to_rav.expect("rav").value_pct, None, "no replacement value");
}

#[tokio::test]
async fn executive_dashboard_scoped_to_one_site() {
    let db = setup().await;
    let d = seed_dataset(&db).await;
    let x = build_executive_dashboard(&db, &filter(30, Some(1)), true, true, true, true, true)
        .await
        .expect("site 1");
    assert_eq!(x.org_node_id, Some(1));
    assert_eq!((x.open_dis.value, x.open_dis.previous_value), (2.0, 0.0));
    assert_eq!(
        x.open_dis.quality_badge.as_deref(),
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE)
    );
    assert_eq!(x.open_wos.value, 1.0);
    assert_eq!(x.total_assets.value, 1.0);
    assert_eq!(x.overdue_items.value, 2.0, "DI2 + PM O1");
    assert_eq!(x.maintenance_cost.value, 100.0);
    assert_eq!(x.di_status_segments.iter().map(|s| s.count).sum::<i64>(), 4);

    let site2 = build_executive_dashboard(&db, &filter(30, Some(2)), true, true, true, true, true)
        .await
        .expect("site 2");
    assert_eq!(site2.open_dis.value, 1.0);
    assert_eq!(site2.open_wos.value, 1.0);
    assert_eq!(site2.pm_overdue.value, 0.0, "the PM plan is on the site 1 asset");
    assert_eq!(site2.maintenance_cost.value, 0.0);
    assert_eq!(site2.reliability_snapshot_count, 1);
    assert!(close(site2.avg_mtbf_hours.value, 500.0));
    assert_eq!(site2.bad_actor_assets.len(), 1);
    assert_eq!(site2.bad_actor_assets[0].equipment_id, d.e2);
}

#[tokio::test]
async fn executive_dashboard_pm_only_overdue_baseline() {
    let db = setup().await;
    seed_dataset(&db).await;
    let x = build_executive_dashboard(&db, &filter(30, None), false, false, false, true, false)
        .await
        .expect("dashboard");
    // O1 is overdue now; 30 days ago no occurrence existed yet.
    assert_eq!((x.overdue_items.value, x.overdue_items.previous_value), (1.0, 0.0));
    assert!(!x.open_dis.available);
    assert!(!x.maintenance_cost.available);
    assert!(x.bad_actor_assets.is_empty());
    assert!(x.reliability_series.is_empty());
    assert!(x.di_status_segments.is_empty());
    assert_eq!(
        x.workload_days
            .iter()
            .map(|w| w.di_created + w.wo_completed)
            .sum::<i64>(),
        0
    );
    assert_eq!(x.workload_days.iter().map(|w| w.pm_due).sum::<i64>(), 2);
}

#[tokio::test]
async fn executive_dashboard_overdue_baseline_counts_each_source_once() {
    let db = setup().await;
    seed_dataset(&db).await;
    let x = build_executive_dashboard(&db, &filter(30, None), true, false, false, true, false)
        .await
        .expect("dashboard");
    assert_eq!(x.overdue_items.value, 3.0);
    // Baseline 30 days ago: DI5 already waiting > 7 days; no PM occurrence existed yet = 1.
    assert_eq!(x.overdue_items.previous_value, 1.0);
}

#[tokio::test]
async fn executive_dashboard_without_permissions_is_all_denied() {
    let db = setup().await;
    seed_dataset(&db).await;
    let x = build_executive_dashboard(&db, &filter(7, None), false, false, false, false, false)
        .await
        .expect("dashboard");
    for k in [
        &x.open_dis,
        &x.open_wos,
        &x.total_assets,
        &x.overdue_items,
        &x.pm_overdue,
        &x.pm_due_today,
        &x.pm_due_next_7d,
        &x.avg_mtbf_hours,
        &x.avg_mttr_hours,
        &x.avg_availability,
        &x.avg_data_quality,
        &x.maintenance_cost,
    ] {
        assert!(!k.available, "{}", k.key);
        assert_eq!(k.value, 0.0, "{}", k.key);
    }
    assert_eq!(
        x.maintenance_cost.catalog_id.as_deref(),
        Some(kpi_definitions::catalog::MAINTENANCE_COST)
    );
    assert!(x.workload_days.is_empty());
    assert_eq!(x.workload_quality_badge, None);
    assert_eq!((x.reliability_snapshot_count, x.reliability_total_events), (0, 0));
    assert!(x.smrp.is_none(), "SMRP needs OT or PM view");
}

#[tokio::test]
async fn executive_dashboard_normalizes_horizon_and_flags_sparse_workload() {
    let db = setup().await;
    let x = build_executive_dashboard(&db, &filter(45, None), true, true, false, true, false)
        .await
        .expect("dashboard");
    assert_eq!(x.horizon_days, 30, "unsupported horizon falls back to 30");
    assert_eq!(x.workload_days.len(), 30);
    assert_eq!(
        x.workload_quality_badge.as_deref(),
        Some(kpi_definitions::quality_badge::SPARSE_WORKLOAD)
    );

    let week = build_executive_dashboard(&db, &filter(7, None), true, false, false, false, false)
        .await
        .expect("dashboard");
    assert_eq!(week.horizon_days, 7);
    assert_eq!(week.workload_days.len(), 7);
    let q = build_executive_dashboard(&db, &filter(90, None), true, false, false, false, false)
        .await
        .expect("dashboard");
    assert_eq!(q.workload_days.len(), 90);
}
