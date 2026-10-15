//! A work order is planned on a shift of its team's Classe horaire (migration 202, `wo::shifts`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::wo::domain::{WoCreateInput, WorkOrder};
use crate::wo::execution::{plan_wo, WoPlanInput};
use crate::wo::shifts::list_wo_shift_options;

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
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn admin(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

/// A weekly Classe horaire (continuous, so the site's rest days do not apply) working
/// `start`–`end` with a 30-minute break, resting on the ISO weekdays in `rest`.
async fn class(db: &DatabaseConnection, code: &str, start: &str, end: &str, rest: &[u32]) -> (i64, i64) {
    exec(
        db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active) \
             SELECT 'shift-{code}', rs.id, '{code}', 'Poste {code}', 99, 1 \
             FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE UPPER(rd.code) = 'ORG.SCHEDULE_CLASS' AND rs.status = 'published' \
             ORDER BY rs.version_no DESC LIMIT 1"
        ),
    )
    .await;
    let class_id = scalar_i64(db, &format!("SELECT id FROM reference_values WHERE code = '{code}'")).await;
    let days: Vec<serde_json::Value> = (1..=7u32)
        .map(|d| {
            serde_json::json!({
                "cycle_day": d, "shift_start": start, "shift_end": end,
                "break_minutes": 30, "is_rest_day": rest.contains(&d),
            })
        })
        .collect();
    exec(
        db,
        &format!(
            "INSERT INTO schedule_class_versions (entity_sync_id, reference_value_id, effective_from, \
             cycle_length_days, rotation_start_date, days_json, is_continuous, utilisation_percent, \
             created_at, updated_at) \
             VALUES ('shift-ver-{code}', {class_id}, '2000-01-01', 7, NULL, '{}', 1, 100.0, datetime('now'), datetime('now'))",
            serde_json::Value::Array(days)
        ),
    )
    .await;
    let version_id = scalar_i64(
        db,
        &format!("SELECT id FROM schedule_class_versions WHERE reference_value_id = {class_id}"),
    )
    .await;
    (class_id, version_id)
}

/// Site, asset, team EQ-SH with three members: two on the day shift (one through the team's
/// default Classe horaire), one on the night shift (rests on Sundays). A third Classe horaire
/// nobody of the team follows.
struct Fixture {
    db: DatabaseConnection,
    team: i64,
    day_version: i64,
    night_version: i64,
    other_version: i64,
}

async fn setup() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'shift-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'shift-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, \
         structure_model_id) VALUES (1, 'shift-site', 'SITE-S', 'Usine', 1, 'active', datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, \
         updated_at) VALUES ('shift-eq', 'POMPE-01', 'Pompe', 'active_in_service', 1, datetime('now'), datetime('now'))",
        "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
         VALUES ('shift-team', 'EQ-SH', 'Mécanique', 'maintenance', 'active', datetime('now'), datetime('now'))",
    ] {
        exec(&db, sql).await;
    }
    let team = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-SH'").await;
    let (day_class, day_version) = class(&db, "JOUR", "06:00", "14:00", &[]).await;
    let (night_class, night_version) = class(&db, "NUIT", "22:00", "06:00", &[7]).await;
    let (_, other_version) = class(&db, "AUTRE", "08:00", "17:00", &[]).await;
    exec(
        &db,
        &format!(
            "INSERT INTO team_schedule_assignments (entity_sync_id, team_id, schedule_reference_value_id, \
             effective_from, created_at, updated_at) \
             VALUES ('shift-link', {team}, {day_class}, '2000-01-01', datetime('now'), datetime('now'))"
        ),
    )
    .await;
    for (code, class) in [
        ("S-1", day_class.to_string()),
        ("S-2", "NULL".to_string()),
        ("S-3", night_class.to_string()),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO personnel (employee_code, full_name, availability_status, primary_team_id, \
                 home_schedule_reference_value_id) VALUES ('{code}', 'Tech {code}', 'available', {team}, {class})"
            ),
        )
        .await;
    }
    Fixture {
        db,
        team,
        day_version,
        night_version,
        other_version,
    }
}

/// A work order in planning, assigned to `team` when given.
async fn work_order(db: &DatabaseConnection, team: Option<i64>) -> WorkOrder {
    let actor = admin(db).await;
    let wo = crate::wo::queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(scalar_i64(db, "SELECT id FROM equipment WHERE asset_id_code = 'POMPE-01'").await),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: Some(1),
            planner_id: None,
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: "Remplacement garniture mécanique".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor,
            requires_permit: Some(false),
        },
    )
    .await
    .expect("wo");
    let wo = crate::wo::workflow::actions::submit::submit_wo(
        db,
        crate::wo::workflow::actions::submit::WoSubmitInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit");
    if let Some(team) = team {
        exec(
            db,
            &format!("UPDATE work_orders SET assigned_team_id = {team} WHERE id = {}", wo.id),
        )
        .await;
    }
    crate::wo::queries::get_work_order(db, wo.id)
        .await
        .expect("get")
        .expect("wo")
}

/// Plans `wo` on `day`, 08:00–12:00, on `version` (or `clear`).
async fn plan(
    db: &DatabaseConnection,
    wo: &WorkOrder,
    day: &str,
    version: Option<i64>,
    clear: bool,
) -> crate::errors::AppResult<WorkOrder> {
    let actor = admin(db).await;
    plan_wo(
        db,
        WoPlanInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            planner_id: actor,
            planned_start: Some(format!("{day}T08:00:00Z")),
            planned_end: Some(format!("{day}T12:00:00Z")),
            shift: None,
            shift_class_version_id: version,
            clear_shift: clear,
            expected_duration_hours: Some(4.0),
            planned_downtime_hours: None,
            urgency_code: None,
        },
    )
    .await
}

const MONDAY: &str = "2026-04-13";
const SUNDAY: &str = "2026-04-19";

#[tokio::test]
async fn shift_options_are_the_team_windows_of_the_day() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let options = list_wo_shift_options(&f.db, wo.id, Some(MONDAY))
        .await
        .expect("options");
    assert_eq!(options.reason, None);
    let windows: Vec<(String, String, i64)> = options
        .options
        .iter()
        .map(|o| (o.shift_start.clone(), o.shift_end.clone(), o.members))
        .collect();
    assert_eq!(
        windows,
        vec![
            ("06:00".to_string(), "14:00".to_string(), 2),
            ("22:00".to_string(), "06:00".to_string(), 1),
        ],
        "the day shift (own class + team default) and the night shift"
    );
    let day = &options.options[0];
    assert_eq!(day.class_version_id, f.day_version);
    assert!((day.net_hours - 7.5).abs() < 1e-9, "8 h minus the 30-minute break");

    // The night shift rests on Sundays: only the day shift is offered.
    let sunday = list_wo_shift_options(&f.db, wo.id, Some(SUNDAY))
        .await
        .expect("options");
    assert_eq!(sunday.options.len(), 1);
    assert_eq!(sunday.options[0].class_version_id, f.day_version);

    // Nothing is offered without a team or a day — never an assumed shift.
    let unassigned = work_order(&f.db, None).await;
    let none = list_wo_shift_options(&f.db, unassigned.id, Some(MONDAY))
        .await
        .expect("options");
    assert!(none.options.is_empty());
    assert_eq!(none.reason.as_deref(), Some("no_team"));
    let no_day = list_wo_shift_options(&f.db, wo.id, None).await.expect("options");
    assert_eq!(no_day.reason.as_deref(), Some("no_day"));
}

#[tokio::test]
async fn a_plan_links_the_version_and_keeps_the_window_it_was_planned_with() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let planned = plan(&f.db, &wo, MONDAY, Some(f.night_version), false)
        .await
        .expect("planned on the night shift");
    assert_eq!(planned.shift_class_version_id, Some(f.night_version));
    assert_eq!(planned.shift_start.as_deref(), Some("22:00"));
    assert_eq!(planned.shift_end.as_deref(), Some("06:00"));
    assert_eq!(planned.shift_class_label.as_deref(), Some("Poste NUIT"));
    assert!(planned.shift.is_none());

    // The timetable changes later: the work order keeps the hours it was planned with.
    exec(
        &f.db,
        &format!(
            "UPDATE schedule_class_versions SET days_json = replace(days_json, '22:00', '21:00') WHERE id = {}",
            f.night_version
        ),
    )
    .await;
    let reloaded = crate::wo::queries::get_work_order(&f.db, wo.id)
        .await
        .expect("get")
        .expect("wo");
    assert_eq!(reloaded.shift_start.as_deref(), Some("22:00"));

    // Moved to a Sunday, when the night shift rests: choose again.
    let refused = plan(&f.db, &reloaded, SUNDAY, None, false)
        .await
        .expect_err("the night shift does not run on Sundays");
    assert!(
        format!("{refused:?}").contains("GATE_WO_SHIFT_NOT_OFFERED"),
        "{refused:?}"
    );
    // Moved with the day shift chosen instead.
    let moved = plan(&f.db, &reloaded, SUNDAY, Some(f.day_version), false)
        .await
        .expect("day shift on Sunday");
    assert_eq!(moved.shift_start.as_deref(), Some("06:00"));
    // Cleared.
    let cleared = plan(&f.db, &moved, SUNDAY, None, true).await.expect("cleared");
    assert!(cleared.shift_class_version_id.is_none() && cleared.shift_start.is_none());
}

#[tokio::test]
async fn a_shift_outside_the_team_or_without_a_team_is_refused() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let refused = plan(&f.db, &wo, MONDAY, Some(f.other_version), false)
        .await
        .expect_err("nobody of the team follows that Classe horaire");
    assert!(
        format!("{refused:?}").contains("GATE_WO_SHIFT_NOT_OFFERED"),
        "{refused:?}"
    );

    let unassigned = work_order(&f.db, None).await;
    let refused = plan(&f.db, &unassigned, MONDAY, Some(f.day_version), false)
        .await
        .expect_err("no team");
    assert!(format!("{refused:?}").contains("GATE_WO_SHIFT_NO_TEAM"), "{refused:?}");
    // Planning without a shift stays possible.
    plan(&f.db, &unassigned, MONDAY, None, false).await.expect("no shift");
}

#[tokio::test]
async fn an_old_shift_code_must_be_replaced_when_re_planning() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    // Planned before migration 202.
    exec(
        &f.db,
        &format!("UPDATE work_orders SET shift = 'morning' WHERE id = {}", wo.id),
    )
    .await;
    let wo = crate::wo::queries::get_work_order(&f.db, wo.id)
        .await
        .expect("get")
        .expect("wo");
    assert_eq!(wo.shift.as_deref(), Some("morning"), "still readable");
    let refused = plan(&f.db, &wo, MONDAY, None, false)
        .await
        .expect_err("re-planning needs a window");
    assert!(
        format!("{refused:?}").contains("GATE_WO_SHIFT_LEGACY_REPLAN"),
        "{refused:?}"
    );
    let replanned = plan(&f.db, &wo, MONDAY, Some(f.day_version), false)
        .await
        .expect("a window of the team");
    assert!(replanned.shift.is_none(), "the old code is dropped");
    assert_eq!(replanned.shift_class_version_id, Some(f.day_version));
}
