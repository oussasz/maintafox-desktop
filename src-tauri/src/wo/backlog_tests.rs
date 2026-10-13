//! Backlog heatmap: planned load vs capacity rules, unscheduled / overdue buckets, blocked work.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::wo::backlog::{get_wo_backlog_heatmap, WoBacklogHeatmapInput};
use crate::wo::domain::WoCreateInput;

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::connect_in_memory().await.expect("in-memory SQLite");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA foreign_keys = ON;".to_string(),
    ))
    .await
    .expect("pragma");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db).await.expect("seeder");
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'bl-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'bl-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'bl-node', 'SITE-BL', 'Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('bl-eq', 'CMP-01', 'Compressor', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("fixture");
    }
    db
}

fn day(offset: i64) -> String {
    (Utc::now().date_naive() + Duration::days(offset))
        .format("%Y-%m-%d")
        .to_string()
}

async fn wo(
    db: &DatabaseConnection,
    title: &str,
    status: &str,
    planned: Option<(i64, i64)>,
    hours: Option<f64>,
) -> i64 {
    let admin: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'admin'".to_string(),
        ))
        .await
        .expect("q")
        .expect("admin")
        .try_get_by_index(0)
        .expect("id");
    let equipment: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE asset_id_code = 'CMP-01'".to_string(),
        ))
        .await
        .expect("q")
        .expect("eq")
        .try_get_by_index(0)
        .expect("id");
    let created = crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(equipment),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: title.into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: admin,
            requires_permit: None,
        },
    )
    .await
    .expect("create");
    let (start, end) = planned.map_or((None, None), |(s, e)| {
        (
            Some(format!("{}T08:00:00Z", day(s))),
            Some(format!("{}T16:00:00Z", day(e))),
        )
    });
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET planned_start = ?, planned_end = ?, expected_duration_hours = ?, \
         status_id = (SELECT id FROM work_order_statuses WHERE code = ?) WHERE id = ?",
        [start.into(), end.into(), hours.into(), status.into(), created.id.into()],
    ))
    .await
    .expect("shape WO");
    created.id
}

#[tokio::test]
async fn heatmap_compares_planned_load_with_capacity_and_lists_backlog() {
    let db = setup().await;
    // Day +1: 10 h planned (6 + 4) and one order without estimate; capacity 8 h → overloaded.
    wo(&db, "A", "ready", Some((1, 1)), Some(6.0)).await;
    wo(&db, "B", "planning", Some((1, 1)), Some(4.0)).await;
    wo(&db, "C", "ready", Some((1, 1)), None).await;
    // Day +2: 3 h planned, under capacity.
    wo(&db, "D", "ready", Some((2, 2)), Some(3.0)).await;
    // Closed and cancelled work never counts.
    wo(&db, "E", "closed", Some((1, 1)), Some(50.0)).await;
    // Unscheduled backlog and an overdue order.
    wo(&db, "F", "planning", None, Some(2.0)).await;
    wo(&db, "G", "ready", Some((-5, -3)), Some(1.0)).await;
    // Blocked on parts.
    let held = wo(&db, "H", "on_hold", None, None).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_delay_segments (work_order_id, started_at, delay_reason_id) \
         VALUES (?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 hours'), \
         (SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
          JOIN reference_domains d ON d.id = rs.domain_id \
          WHERE UPPER(d.code) = 'WORK.DELAY_REASONS' AND rv.code = 'no_parts' LIMIT 1))",
        [held.into()],
    ))
    .await
    .expect("delay");

    // Without capacity rules nothing is called overloaded.
    let no_cap = get_wo_backlog_heatmap(
        &db,
        WoBacklogHeatmapInput {
            start_date: None,
            weeks: 1,
        },
    )
    .await
    .expect("heatmap");
    assert!(!no_cap.capacity_configured);
    assert!(no_cap.days.iter().all(|d| !d.overloaded && d.capacity_hours.is_none()));

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at) \
         VALUES ('bl-crew', 'CREW-BL', 'Crew', 'maintenance', 1, 'active', datetime('now'), datetime('now'))"
            .to_string(),
    ))
    .await
    .expect("crew");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO capacity_rules (team_id, effective_start, available_hours_per_day, max_overtime_hours_per_day) \
         VALUES ((SELECT id FROM teams WHERE code = 'CREW-BL'), ?, 7.0, 1.0)",
        [day(-30).into()],
    ))
    .await
    .expect("capacity");
    let map = get_wo_backlog_heatmap(
        &db,
        WoBacklogHeatmapInput {
            start_date: None,
            weeks: 2,
        },
    )
    .await
    .expect("heatmap");
    assert_eq!(map.days.len(), 14);
    let d1 = &map.days[1];
    assert_eq!(d1.date, day(1));
    assert!((d1.planned_hours - 10.0).abs() < 1e-9);
    assert_eq!((d1.wo_count, d1.unestimated_count), (3, 1));
    assert_eq!(d1.capacity_hours, Some(8.0));
    assert!(d1.overloaded);
    let d2 = &map.days[2];
    assert!((d2.planned_hours - 3.0).abs() < 1e-9);
    assert!(!d2.overloaded);

    assert_eq!(map.unscheduled.wo_count, 1);
    assert!((map.unscheduled.estimated_hours - 2.0).abs() < 1e-9);
    assert_eq!(map.overdue.wo_count, 1);
    assert_eq!(map.blocked.len(), 1);
    assert_eq!(map.blocked[0].category, "parts");
    assert!(map.blocked[0].waiting_hours > 1.9);

    assert!(get_wo_backlog_heatmap(
        &db,
        WoBacklogHeatmapInput {
            start_date: None,
            weeks: 13
        }
    )
    .await
    .is_err());
}
