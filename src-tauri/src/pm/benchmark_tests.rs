//! PM benchmark gaps: tolerance windows and timing, calendar months, fan-out by location,
//! whichever comes first, meter rollover / replacement, compliance %, revision from a finding,
//! reservations of planned parts.

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, ExecutePmOccurrenceInput, GeneratePmOccurrencesInput,
    PmComplianceInput, PmOccurrence, PmOccurrenceFilter, PublishPmPlanVersionInput, TransitionPmOccurrenceInput,
    TransitionPmPlanLifecycleInput,
};
use crate::pm::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db)
        .await
        .expect("seed system data");
    crate::locale::write_locale_config(&db, "locale.default_language", "fr")
        .await
        .expect("tenant locale");
    db
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "v")
        .expect("v")
}

/// Org tree `/1/` (site) → `/1/2/` (workshop), a separate root `/3/`; equipment 1 in the
/// workshop, 2 at the site, 3 elsewhere.
async fn seed_plant(db: &DatabaseConnection) {
    exec(
        db,
        "INSERT OR IGNORE INTO org_structure_models
            (id, entity_sync_id, version_number, status, description, activated_at, created_at, updated_at)
         VALUES (1, 'bm_model', 1, 'active', 'model', strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
    )
    .await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_node_types
            (id, entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, is_active, created_at, updated_at)
         VALUES (1, 'bm_type', 1, 'SITE', 'Site', 1, 1, 1, 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
    )
    .await;
    for (id, code, parent, path, depth) in [
        (1, "SITE", "NULL", "/1/", 0),
        (2, "ATELIER", "1", "/1/2/", 1),
        (3, "AUTRE", "NULL", "/3/", 0),
    ] {
        exec(
            db,
            &format!(
                "INSERT OR IGNORE INTO org_nodes
                    (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
                     created_at, updated_at, row_version, structure_model_id)
                 VALUES ({id}, 'bm_node_{id}', '{code}', '{code}', 1, {parent}, '{path}', {depth}, 'active',
                         strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 1)"
            ),
        )
        .await;
    }
    for (id, code, node) in [(1, "CMP-01", 2), (2, "CMP-02", 1), (3, "CMP-03", 3)] {
        exec(
            db,
            &format!(
                "INSERT OR IGNORE INTO equipment
                    (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
                 VALUES ({id}, 'bm_eq_{id}', '{code}', 'Compresseur {id}', 'active_in_service', {node},
                         strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            ),
        )
        .await;
    }
}

struct PlanSpec<'a> {
    code: &'a str,
    scope_type: &'a str,
    scope_id: Option<i64>,
    strategy: &'a str,
    trigger: String,
    effective_from: &'a str,
    parts: Option<&'a str>,
}

/// Active plan with a published version; returns the plan id.
async fn active_plan(db: &DatabaseConnection, spec: PlanSpec<'_>) -> i64 {
    let plan = queries::create_pm_plan(
        db,
        CreatePmPlanInput {
            code: spec.code.to_string(),
            title: format!("Plan {}", spec.code),
            description: None,
            asset_scope_type: spec.scope_type.to_string(),
            asset_scope_id: spec.scope_id,
            strategy_type: spec.strategy.to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("create plan");
    let mut rv = plan.row_version;
    for next in ["proposed", "approved"] {
        rv = queries::transition_pm_plan_lifecycle(
            db,
            TransitionPmPlanLifecycleInput {
                plan_id: plan.id,
                expected_row_version: rv,
                next_status: next.to_string(),
            },
        )
        .await
        .expect("lifecycle")
        .row_version;
    }
    let version = queries::create_pm_plan_version(
        db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: spec.effective_from.to_string(),
            effective_to: None,
            trigger_definition_json: spec.trigger,
            task_package_json: None,
            required_parts_json: spec.parts.map(str::to_string),
            required_skills_json: Some("[]".to_string()),
            required_tools_json: None,
            estimated_duration_hours: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("create version");
    queries::publish_pm_plan_version(
        db,
        PublishPmPlanVersionInput {
            version_id: version.id,
            expected_row_version: version.row_version,
        },
    )
    .await
    .expect("publish");
    plan.id
}

async fn generate(db: &DatabaseConnection, plan_id: i64, as_of: &str, horizon_days: i64) -> i64 {
    queries::generate_pm_occurrences(
        db,
        GeneratePmOccurrencesInput {
            as_of: Some(as_of.to_string()),
            horizon_days: Some(horizon_days),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate")
    .generated_count
}

async fn occurrences(db: &DatabaseConnection, plan_id: i64) -> Vec<PmOccurrence> {
    let mut list = queries::list_pm_occurrences(
        db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list");
    list.sort_by(|a, b| a.due_at.cmp(&b.due_at).then(a.equipment_id.cmp(&b.equipment_id)));
    list
}

async fn to_in_progress(db: &DatabaseConnection, mut occ: PmOccurrence) -> PmOccurrence {
    for next in ["ready_for_scheduling", "scheduled", "in_progress"] {
        occ = queries::transition_pm_occurrence(
            db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: next.to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect("advance");
    }
    occ
}

async fn complete(db: &DatabaseConnection, occ: PmOccurrence) -> crate::pm::domain::ExecutePmOccurrenceResult {
    let occ = to_in_progress(db, occ).await;
    queries::execute_pm_occurrence(
        db,
        ExecutePmOccurrenceInput {
            occurrence_id: occ.id,
            expected_occurrence_row_version: occ.row_version,
            execution_result: "completed_no_findings".to_string(),
            note: None,
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: None,
        },
    )
    .await
    .expect("execute")
}

#[tokio::test]
async fn tolerance_window_is_stored_and_late_work_is_marked_late() {
    let db = setup_db().await;
    seed_plant(&db).await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-TOL",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "fixed",
            trigger: r#"{"interval_unit":"day","interval_value":10,"tolerance":{"unit":"days","early":1,"late":2}}"#
                .to_string(),
            effective_from: "2026-01-01T00:00:00Z",
            parts: None,
        },
    )
    .await;
    assert_eq!(generate(&db, plan_id, "2026-01-10T00:00:00Z", 5).await, 1);
    let occ = occurrences(&db, plan_id).await.remove(0);
    assert_eq!(occ.due_at.as_deref(), Some("2026-01-11T00:00:00Z"));
    assert_eq!(occ.window_start_at.as_deref(), Some("2026-01-10T00:00:00Z"));
    assert_eq!(occ.window_end_at.as_deref(), Some("2026-01-13T23:59:59Z"));
    assert_eq!(occ.equipment_id, Some(1));
    // Recorded today, long after the window.
    let result = complete(&db, occ).await;
    assert_eq!(result.execution.timing_status.as_deref(), Some("late"));
}

#[tokio::test]
async fn monthly_plan_keeps_the_day_of_month() {
    let db = setup_db().await;
    seed_plant(&db).await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-MONTH",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "fixed",
            trigger: r#"{"interval_unit":"month","interval_value":1}"#.to_string(),
            effective_from: "2026-10-15T00:00:00Z",
            parts: None,
        },
    )
    .await;
    generate(&db, plan_id, "2026-10-15T00:00:00Z", 95).await;
    let dues: Vec<String> = occurrences(&db, plan_id)
        .await
        .into_iter()
        .filter_map(|o| o.due_at)
        .collect();
    assert_eq!(
        dues,
        vec![
            "2026-10-15T00:00:00Z",
            "2026-11-15T00:00:00Z",
            "2026-12-15T00:00:00Z",
            "2027-01-15T00:00:00Z"
        ]
    );
}

#[tokio::test]
async fn location_plan_fans_out_one_occurrence_per_asset() {
    let db = setup_db().await;
    seed_plant(&db).await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-SITE",
            scope_type: "location",
            scope_id: Some(1),
            strategy: "fixed",
            trigger: r#"{"interval_unit":"day","interval_value":30}"#.to_string(),
            effective_from: "2026-01-10T00:00:00Z",
            parts: None,
        },
    )
    .await;
    let preview = crate::pm::plan_tools::preview_pm_scope_assets(&db, "location", 1)
        .await
        .expect("preview");
    assert_eq!(
        preview.iter().map(|a| a.asset_code.as_str()).collect::<Vec<_>>(),
        vec!["CMP-01", "CMP-02"],
        "the workshop below the site is included, the other root is not"
    );
    assert_eq!(generate(&db, plan_id, "2026-01-10T00:00:00Z", 5).await, 2);
    assert_eq!(
        generate(&db, plan_id, "2026-01-10T00:00:00Z", 5).await,
        0,
        "idempotent per asset"
    );
    let occs = occurrences(&db, plan_id).await;
    assert_eq!(
        occs.iter()
            .map(|o| o.asset_code.clone().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["CMP-01", "CMP-02"]
    );
    // The OT of one asset's occurrence is for that asset.
    let occ = occs[1].clone();
    let ready = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(true),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("ready with OT");
    let wo = crate::wo::queries::get_work_order(&db, ready.linked_work_order_id.expect("OT"))
        .await
        .expect("get")
        .expect("OT");
    assert_eq!(wo.equipment_id, Some(2));
    assert_eq!(wo.title, "Plan PM-SITE — CMP-02");
}

async fn seed_meter(db: &DatabaseConnection, reading: f64, read_at: &str, rollover: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO equipment_meters
                (id, entity_sync_id, equipment_id, name, meter_type, unit, current_reading, last_read_at,
                 expected_rate_per_day, rollover_value, is_primary, is_active, created_at, updated_at)
             VALUES (9, 'bm_meter', 1, 'Heures marche', 'HOURS', 'h', {reading}, '{read_at}', 0, {rollover}, 1, 1,
                     strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
        ),
    )
    .await;
}

async fn read_meter(db: &DatabaseConnection, value: f64, at: &str, kind: Option<&str>) -> f64 {
    crate::pm::plan_tools::record_pm_meter_reading(
        db,
        crate::pm::domain::RecordPmMeterReadingInput {
            meter_id: 9,
            reading_value: value,
            reading_at: Some(at.to_string()),
            kind: kind.map(str::to_string),
        },
        1,
    )
    .await
    .expect("reading")
    .lifetime_reading
}

#[tokio::test]
async fn operating_hours_interval_plan_generates_from_the_reading_at_start() {
    let db = setup_db().await;
    seed_plant(&db).await;
    seed_meter(&db, 0.0, "2025-12-01T00:00:00Z", "NULL").await;
    read_meter(&db, 500.0, "2025-12-31T00:00:00Z", None).await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-HOURS",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "meter",
            trigger: r#"{"meter_source":"operating_hours","interval_value":100}"#.to_string(),
            effective_from: "2026-01-01T00:00:00Z",
            parts: None,
        },
    )
    .await;
    // 500 h at the start; due at 600 h. Each reading re-evaluates the asset's plans.
    read_meter(&db, 580.0, "2026-01-05T00:00:00Z", None).await;
    assert!(occurrences(&db, plan_id).await.is_empty(), "580 h < 600 h");
    read_meter(&db, 620.0, "2026-01-08T00:00:00Z", None).await;
    let occs = occurrences(&db, plan_id).await;
    assert_eq!(occs.len(), 1, "the reading crossing 600 h generated the occurrence");
    assert_eq!(generate(&db, plan_id, "2026-01-08T00:00:00Z", 7).await, 0, "not twice");
    let occ = occs[0].clone();
    assert_eq!(occ.due_meter_value, Some(600.0));
    assert_eq!(
        occ.due_at.as_deref(),
        Some("2026-01-08T00:00:00Z"),
        "due when the meter was read"
    );
}

#[tokio::test]
async fn rollover_and_replacement_keep_counting_usage() {
    let db = setup_db().await;
    seed_plant(&db).await;
    seed_meter(&db, 9900.0, "2026-01-01T00:00:00Z", "9999").await;
    assert_eq!(read_meter(&db, 9990.0, "2026-01-02T00:00:00Z", None).await, 9990.0);
    // Counter passed 9999 and restarted: 9 h to the top + 20 h.
    assert_eq!(read_meter(&db, 20.0, "2026-01-03T00:00:00Z", None).await, 10019.0);
    // New meter fitted, reads 5: no usage is invented across the swap.
    assert_eq!(
        read_meter(&db, 5.0, "2026-01-04T00:00:00Z", Some("replacement")).await,
        10019.0
    );
    assert_eq!(read_meter(&db, 30.0, "2026-01-05T00:00:00Z", None).await, 10044.0);
    // A lower reading that is neither is refused.
    let err = crate::pm::plan_tools::record_pm_meter_reading(
        &db,
        crate::pm::domain::RecordPmMeterReadingInput {
            meter_id: 9,
            reading_value: 10.0,
            reading_at: Some("2026-01-06T00:00:00Z".to_string()),
            kind: Some("normal".to_string()),
        },
        1,
    )
    .await;
    assert!(err.is_err());

    // Usage PM counts in lifetime hours: every 50 h from 9980 h → due at 10030 h.
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-LIFE",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "meter",
            trigger: r#"{"asset_meter_id":9,"interval_value":50,"start_reading":9980}"#.to_string(),
            effective_from: "2026-01-01T00:00:00Z",
            parts: None,
        },
    )
    .await;
    assert_eq!(generate(&db, plan_id, "2026-01-05T00:00:00Z", 7).await, 1);
    assert_eq!(occurrences(&db, plan_id).await[0].due_meter_value, Some(10030.0));
}

#[tokio::test]
async fn whichever_comes_first_brings_the_calendar_occurrence_forward() {
    let db = setup_db().await;
    seed_plant(&db).await;
    seed_meter(&db, 1050.0, "2026-01-10T00:00:00Z", "NULL").await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-OR",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "floating",
            trigger: r#"{"interval_unit":"month","interval_value":1,"meter":{"meter_type":"HOURS","interval_value":100,"start_reading":1000}}"#
                .to_string(),
            effective_from: "2026-01-01T00:00:00Z",
            parts: None,
        },
    )
    .await;
    assert_eq!(generate(&db, plan_id, "2026-01-10T00:00:00Z", 30).await, 1);
    let occ = occurrences(&db, plan_id).await.remove(0);
    assert_eq!(
        occ.due_at.as_deref(),
        Some("2026-02-01T00:00:00Z"),
        "calendar date first"
    );
    assert_eq!(occ.due_meter_value, None);

    // 1100 h reached on 12 January, before the calendar date.
    exec(
        &db,
        "UPDATE equipment_meters SET current_reading = 1120, last_read_at = '2026-01-12T08:00:00Z' WHERE id = 9",
    )
    .await;
    assert_eq!(generate(&db, plan_id, "2026-01-12T09:00:00Z", 30).await, 0);
    let occs = occurrences(&db, plan_id).await;
    assert_eq!(occs.len(), 1, "the same occurrence, not a second one");
    assert_eq!(occs[0].due_at.as_deref(), Some("2026-01-12T08:00:00Z"));
    assert_eq!(occs[0].due_meter_value, Some(1100.0));

    // Completed: the reading is kept as the next anchor (1120 → next at 1220 h).
    let result = complete(&db, occs[0].clone()).await;
    assert_eq!(result.execution.meter_reading, Some(1120.0));
}

#[tokio::test]
async fn compliance_counts_work_inside_the_window_over_work_due() {
    let db = setup_db().await;
    seed_plant(&db).await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-KPI",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "fixed",
            trigger: r#"{"interval_unit":"day","interval_value":365}"#.to_string(),
            effective_from: "2030-01-01T00:00:00Z",
            parts: None,
        },
    )
    .await;
    let version_id = scalar_i64(
        &db,
        &format!("SELECT current_version_id AS v FROM pm_plans WHERE id = {plan_id}"),
    )
    .await;
    // (due day, status, window end, executed_at, stored timing)
    let rows = [
        (
            "2026-01-05",
            "completed",
            Some("2026-01-06T23:59:59Z"),
            Some("2026-01-06T10:00:00Z"),
            Some("on_time"),
        ),
        (
            "2026-01-10",
            "completed",
            Some("2026-01-10T23:59:59Z"),
            Some("2026-01-15T10:00:00Z"),
            Some("late"),
        ),
        (
            "2026-01-15",
            "completed",
            Some("2026-01-15T23:59:59Z"),
            Some("2026-01-01T10:00:00Z"),
            Some("early"),
        ),
        ("2026-01-20", "scheduled", Some("2026-01-20T23:59:59Z"), None, None),
        ("2026-01-25", "cancelled", Some("2026-01-25T23:59:59Z"), None, None),
        (
            "2026-02-02",
            "completed",
            Some("2026-02-02T23:59:59Z"),
            Some("2026-02-02T10:00:00Z"),
            Some("on_time"),
        ),
        // Legacy completion without window nor timing, done on its due day.
        ("2026-01-28", "completed", None, Some("2026-01-28T10:00:00Z"), None),
    ];
    for (i, (day, status, end, executed, timing)) in rows.iter().enumerate() {
        let end_sql = end.map(|e| format!("'{e}'")).unwrap_or_else(|| "NULL".into());
        exec(
            &db,
            &format!(
                "INSERT INTO pm_occurrences
                    (entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, status, equipment_id, window_end_at)
                 VALUES ('bm_kpi_{i}', {plan_id}, {version_id}, 'kpi:{i}', '{day}T00:00:00Z', '{status}', 1, {end_sql})"
            ),
        )
        .await;
        if let Some(executed) = executed {
            let timing_sql = timing.map(|t| format!("'{t}'")).unwrap_or_else(|| "NULL".into());
            exec(
                &db,
                &format!(
                    "INSERT INTO pm_executions (pm_occurrence_id, execution_result, executed_at, timing_status, created_at)
                     SELECT id, 'completed_no_findings', '{executed}', {timing_sql}, '{executed}'
                     FROM pm_occurrences WHERE entity_sync_id = 'bm_kpi_{i}'"
                ),
            )
            .await;
        }
    }
    let report = crate::pm::compliance::get_pm_compliance(
        &db,
        PmComplianceInput {
            from: "2026-01-01T00:00:00Z".to_string(),
            to: "2026-02-01T00:00:00Z".to_string(),
            pm_plan_id: Some(plan_id),
        },
    )
    .await
    .expect("compliance");
    // Due in January, not cancelled: 5, 10, 15, 20, 28 → 5. On time: 5 and 28 (legacy, due day).
    assert_eq!(report.due_count, 5);
    assert_eq!(report.on_time_count, 2);
    assert_eq!(report.late_count, 1);
    assert_eq!(report.early_count, 1);
    assert_eq!(report.not_done_count, 1);
    assert_eq!(report.compliance_pct, Some(40.0));
}

#[tokio::test]
async fn revision_from_a_finding_creates_a_draft_with_the_finding_as_reason() {
    let db = setup_db().await;
    seed_plant(&db).await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-REV",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "fixed",
            trigger: r#"{"interval_unit":"day","interval_value":7}"#.to_string(),
            effective_from: "2026-01-01T00:00:00Z",
            parts: None,
        },
    )
    .await;
    generate(&db, plan_id, "2026-01-10T00:00:00Z", 5).await;
    let occ = to_in_progress(&db, occurrences(&db, plan_id).await.remove(0)).await;
    let result = queries::execute_pm_occurrence(
        &db,
        ExecutePmOccurrenceInput {
            occurrence_id: occ.id,
            expected_occurrence_row_version: occ.row_version,
            execution_result: "completed_with_findings".to_string(),
            note: None,
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: Some(vec![crate::pm::domain::PmExecutionFindingInput {
                finding_type: "vibration".to_string(),
                severity: Some("high".to_string()),
                description: "Graissage hebdomadaire insuffisant".to_string(),
                create_follow_up_di: None,
                create_follow_up_work_order: None,
                follow_up_work_order_type_id: None,
            }]),
        },
    )
    .await
    .expect("execute");
    let finding_id = result.findings[0].id;
    let draft = crate::pm::plan_tools::create_pm_revision_from_finding(&db, finding_id)
        .await
        .expect("revision");
    assert_eq!(draft.status, "draft");
    assert_eq!(draft.version_no, 2);
    assert_eq!(
        draft.change_reason.as_deref(),
        Some("Graissage hebdomadaire insuffisant")
    );
    assert_eq!(
        draft.trigger_definition_json,
        r#"{"interval_unit":"day","interval_value":7}"#
    );
    let again = crate::pm::plan_tools::create_pm_revision_from_finding(&db, finding_id)
        .await
        .expect("again");
    assert_eq!(again.id, draft.id);
    let source = scalar_i64(
        &db,
        &format!("SELECT COUNT(*) AS v FROM pm_plan_versions WHERE id = {} AND source_type = 'pm_finding' AND source_id = {finding_id}", draft.id),
    )
    .await;
    assert_eq!(source, 1);
}

#[tokio::test]
async fn planned_parts_are_reserved_where_stock_covers_them() {
    let db = setup_db().await;
    seed_plant(&db).await;
    exec(
        &db,
        "INSERT INTO articles (article_code, article_name, unit_value_id, is_active)
         SELECT 'FLT-10', 'Filtre à huile', (SELECT id FROM lookup_values ORDER BY id LIMIT 1), 1",
    )
    .await;
    exec(
        &db,
        "INSERT INTO warehouses (id, code, name) VALUES (50, 'MAG', 'Magasin')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO stock_locations (id, warehouse_id, code, name) VALUES (60, 50, 'A1', 'Allée 1'), (61, 50, 'A2', 'Allée 2')",
    )
    .await;
    exec(
        &db,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty)
         SELECT id, 50, 60, 1, 0, 1 FROM articles WHERE article_code = 'FLT-10'",
    )
    .await;
    exec(
        &db,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty)
         SELECT id, 50, 61, 5, 0, 5 FROM articles WHERE article_code = 'FLT-10'",
    )
    .await;
    let plan_id = active_plan(
        &db,
        PlanSpec {
            code: "PM-PARTS",
            scope_type: "equipment",
            scope_id: Some(1),
            strategy: "fixed",
            trigger: r#"{"interval_unit":"day","interval_value":7}"#.to_string(),
            effective_from: "2026-01-01T00:00:00Z",
            parts: Some(r#"[{"article_code":"FLT-10","quantity":2}]"#),
        },
    )
    .await;
    generate(&db, plan_id, "2026-01-10T00:00:00Z", 5).await;
    let occ = occurrences(&db, plan_id).await.remove(0);
    let ready = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(true),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("ready with OT");
    let parts = crate::wo::parts::list_wo_parts(&db, ready.linked_work_order_id.expect("OT"))
        .await
        .expect("parts");
    assert_eq!(parts.len(), 1);
    assert_eq!(
        parts[0].stock_location_id,
        Some(61),
        "the location that covers the 2 filters"
    );
    assert!(parts[0].reservation_id.is_some());
    assert!((parts[0].quantity_reserved - 2.0).abs() < f64::EPSILON);
}

#[test]
fn meter_offset_rules() {
    use crate::assets::meters::{next_meter_offset, MeterReadingKind};
    assert_eq!(
        next_meter_offset(Some(9990.0), 0.0, 20.0, Some(9999.0), None).expect("rollover"),
        (MeterReadingKind::Rollover, 9999.0)
    );
    assert_eq!(
        next_meter_offset(
            Some(20.0),
            9999.0,
            5.0,
            Some(9999.0),
            Some(MeterReadingKind::Replacement)
        )
        .expect("replacement"),
        (MeterReadingKind::Replacement, 10014.0)
    );
    assert!(next_meter_offset(Some(100.0), 0.0, 50.0, None, None).is_err());
    assert_eq!(
        next_meter_offset(None, 3.0, 50.0, None, None).expect("first"),
        (MeterReadingKind::Normal, 3.0)
    );
}
