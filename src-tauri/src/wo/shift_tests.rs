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
    /// A team on nights only, and a team working the same day shift as `team`.
    night_team: i64,
    day_team: i64,
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
    // Two more teams: one on nights only, one working the same day shift.
    let mut other_teams = Vec::new();
    for (team_code, member, class) in [("EQ-NUIT", "N-1", night_class), ("EQ-JOUR", "J-1", day_class)] {
        exec(
            &db,
            &format!(
                "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
                 VALUES ('shift-{team_code}', '{team_code}', 'Équipe {team_code}', 'maintenance', 'active', \
                 datetime('now'), datetime('now'))"
            ),
        )
        .await;
        let id = scalar_i64(&db, &format!("SELECT id FROM teams WHERE code = '{team_code}'")).await;
        exec(
            &db,
            &format!(
                "INSERT INTO personnel (employee_code, full_name, availability_status, primary_team_id, \
                 home_schedule_reference_value_id) VALUES ('{member}', 'Tech {member}', 'available', {id}, {class})"
            ),
        )
        .await;
        other_teams.push(id);
    }
    Fixture {
        db,
        team,
        night_team: other_teams[0],
        day_team: other_teams[1],
        day_version,
        night_version,
        other_version,
    }
}

/// Emergency reassignment of `wo` to `team`, with an optional new shift choice.
async fn reassign(
    db: &DatabaseConnection,
    wo: &WorkOrder,
    team: i64,
    version: Option<i64>,
    clear: bool,
) -> crate::errors::AppResult<WorkOrder> {
    crate::wo::workflow::actions::assign::emergency_reassign(
        db,
        crate::wo::workflow::actions::assign::WoEmergencyReassignInput {
            wo_id: wo.id,
            actor_id: admin(db).await,
            expected_row_version: wo.row_version,
            assigned_team_id: Some(team),
            primary_responsible_id: None,
            override_reason: "Équipe initiale indisponible".into(),
            shift_class_version_id: version,
            clear_shift: clear,
        },
    )
    .await
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
            planned_crew_size: None,
            clear_crew_size: false,
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
    let options = list_wo_shift_options(&f.db, wo.id, Some(MONDAY), None)
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
    let sunday = list_wo_shift_options(&f.db, wo.id, Some(SUNDAY), None)
        .await
        .expect("options");
    assert_eq!(sunday.options.len(), 1);
    assert_eq!(sunday.options[0].class_version_id, f.day_version);

    // Nothing is offered without a team or a day — never an assumed shift.
    let unassigned = work_order(&f.db, None).await;
    let none = list_wo_shift_options(&f.db, unassigned.id, Some(MONDAY), None)
        .await
        .expect("options");
    assert!(none.options.is_empty());
    assert_eq!(none.reason.as_deref(), Some("no_team"));
    let no_day = list_wo_shift_options(&f.db, wo.id, None, None).await.expect("options");
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

/// Normal assignment (`assign_wo`) of `wo` to `team`.
async fn assign(db: &DatabaseConnection, wo: &WorkOrder, team: i64) -> crate::errors::AppResult<WorkOrder> {
    crate::wo::workflow::actions::assign::save_assignment(
        db,
        crate::wo::workflow::actions::assign::WoAssignSaveInput {
            wo_id: wo.id,
            actor_id: admin(db).await,
            expected_row_version: wo.row_version,
            assigned_team_id: Some(team),
            primary_responsible_id: None,
            scheduled_at: None,
            override_reason: "Changement d'équipe planifié".into(),
        },
    )
    .await
}

async fn reload(db: &DatabaseConnection, wo_id: i64) -> WorkOrder {
    crate::wo::queries::get_work_order(db, wo_id)
        .await
        .expect("get")
        .expect("wo")
}

/// HIGH 2 — normal assignment: a new team that does not work the chosen shift is refused; the
/// planner chooses one of its shifts (the planning tab lists them) or none.
#[tokio::test]
async fn a_normal_assignment_to_a_team_that_does_not_work_the_shift_is_refused() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let planned = plan(&f.db, &wo, MONDAY, Some(f.day_version), false)
        .await
        .expect("planned on the day shift");

    let refused = assign(&f.db, &planned, f.night_team)
        .await
        .expect_err("the night team does not work the day shift");
    assert!(
        format!("{refused:?}").contains("GATE_WO_SHIFT_TEAM_CHANGED"),
        "{refused:?}"
    );
    let unchanged = reload(&f.db, wo.id).await;
    assert_eq!(unchanged.assigned_team_id, Some(f.team));
    assert_eq!(unchanged.shift_class_version_id, Some(f.day_version));

    // The planning tab offers the new team's shifts…
    let offered = list_wo_shift_options(&f.db, wo.id, Some(MONDAY), Some(f.night_team))
        .await
        .expect("options");
    assert_eq!(
        offered.options.iter().map(|o| o.class_version_id).collect::<Vec<_>>(),
        vec![f.night_version]
    );
    // …and the reassignment goes through with one of them, or with no shift.
    let moved = reassign(&f.db, &unchanged, f.night_team, Some(f.night_version), false)
        .await
        .expect("new team with its shift");
    assert_eq!(moved.assigned_team_id, Some(f.night_team));
    assert_eq!(moved.shift_class_version_id, Some(f.night_version));
    assert_eq!(moved.shift_start.as_deref(), Some("22:00"));
    let back = reassign(&f.db, &moved, f.team, None, true).await.expect("cleared");
    assert!(back.shift_class_version_id.is_none() && back.shift_start.is_none());
}

/// HIGH 2 — an emergency reassignment is never blocked: the shift is cleared and the work order
/// records that it must be chosen again.
#[tokio::test]
async fn an_emergency_reassignment_clears_a_shift_the_new_team_does_not_work_and_says_so() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let planned = plan(&f.db, &wo, MONDAY, Some(f.day_version), false)
        .await
        .expect("planned on the day shift");
    let moved = reassign(&f.db, &planned, f.night_team, None, false)
        .await
        .expect("an emergency is not blocked");
    assert_eq!(moved.assigned_team_id, Some(f.night_team));
    assert!(moved.shift_class_version_id.is_none() && moved.shift_start.is_none());
    let note: String =
        f.db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT payload_json FROM wo_action_events WHERE wo_id = {} AND action_code = 'shift_cleared'",
                wo.id
            ),
        ))
        .await
        .expect("query")
        .expect("the work order says the shift must be chosen again")
        .try_get("", "payload_json")
        .expect("payload");
    assert!(note.contains("\"shift_to_rechoose\":true"), "{note}");
    assert!(note.contains("\"shift_start\":\"06:00\""), "{note}");
}

#[tokio::test]
async fn a_new_team_working_the_same_shift_keeps_it() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let planned = plan(&f.db, &wo, MONDAY, Some(f.day_version), false)
        .await
        .expect("planned on the day shift");
    let moved = assign(&f.db, &planned, f.day_team)
        .await
        .expect("the new team works the same day shift");
    assert_eq!(moved.assigned_team_id, Some(f.day_team));
    assert_eq!(moved.shift_class_version_id, Some(f.day_version));
    assert_eq!(moved.shift_start.as_deref(), Some("06:00"));
    assert_eq!(moved.shift_end.as_deref(), Some("14:00"));
}

/// HIGH 1 / MEDIUM 6 — a save that does not touch the shift keeps it as planned, even when the
/// team no longer works it or the timetable changed: editing the hours is never blocked.
#[tokio::test]
async fn a_save_that_does_not_touch_the_shift_keeps_it() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let planned = plan(&f.db, &wo, MONDAY, Some(f.night_version), false)
        .await
        .expect("planned on the night shift");
    // The night worker leaves the team, and the night timetable is edited.
    exec(
        &f.db,
        &format!(
            "UPDATE personnel SET primary_team_id = {} WHERE employee_code = 'S-3'",
            f.night_team
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "UPDATE schedule_class_versions SET days_json = replace(days_json, '22:00', '21:00') WHERE id = {}",
            f.night_version
        ),
    )
    .await;
    let saved = plan(&f.db, &planned, MONDAY, None, false)
        .await
        .expect("an hours edit on the same day goes through");
    assert_eq!(saved.shift_class_version_id, Some(f.night_version));
    assert_eq!(
        saved.shift_start.as_deref(),
        Some("22:00"),
        "the window it was planned with"
    );
}

/// MEDIUM 3 — an old shift code survives a save that does not move the work order; moving it asks
/// for a window (or none).
#[tokio::test]
async fn an_old_shift_code_is_kept_until_the_work_order_moves() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    plan(&f.db, &wo, MONDAY, None, false).await.expect("planned");
    exec(
        &f.db,
        &format!("UPDATE work_orders SET shift = 'night' WHERE id = {}", wo.id),
    )
    .await;
    let planned = reload(&f.db, wo.id).await;
    let saved = plan(&f.db, &planned, MONDAY, None, false)
        .await
        .expect("same day: the old code is kept");
    assert_eq!(saved.shift.as_deref(), Some("night"));
    let refused = plan(&f.db, &saved, "2026-04-14", None, false)
        .await
        .expect_err("moved: choose a window");
    assert!(
        format!("{refused:?}").contains("GATE_WO_SHIFT_LEGACY_REPLAN"),
        "{refused:?}"
    );
}

/// MEDIUM 4 — an early-morning start also offers the previous night's shift still running.
#[tokio::test]
async fn an_early_morning_start_offers_the_previous_night_shift() {
    let f = setup().await;
    let wo = work_order(&f.db, Some(f.team)).await;
    // Tuesday 02:00 (local, as typed): Monday's night shift is still running.
    let early = list_wo_shift_options(&f.db, wo.id, Some("2026-04-14T02:00"), None)
        .await
        .expect("options");
    let night = early
        .options
        .iter()
        .find(|o| o.class_version_id == f.night_version)
        .expect("the night shift");
    assert!(night.starts_previous_day, "{early:?}");
    assert_eq!(early.day.as_deref(), Some("2026-04-14"));
    // At 08:00 the same night shift is the one starting that evening.
    let morning = list_wo_shift_options(&f.db, wo.id, Some("2026-04-14T08:00"), None)
        .await
        .expect("options");
    let night = morning
        .options
        .iter()
        .find(|o| o.class_version_id == f.night_version)
        .expect("the night shift");
    assert!(!night.starts_previous_day);
}

/// MEDIUM 5 — moving across a new version of the same Classe horaire keeps the class and takes the
/// version in force on the new day.
#[tokio::test]
async fn a_move_across_a_new_version_keeps_the_classe_horaire() {
    let f = setup().await;
    let night_class = scalar_i64(&f.db, "SELECT id FROM reference_values WHERE code = 'NUIT'").await;
    let days: Vec<serde_json::Value> = (1..=7u32)
        .map(|d| {
            serde_json::json!({
                "cycle_day": d, "shift_start": "21:00", "shift_end": "05:00",
                "break_minutes": 30, "is_rest_day": d == 7,
            })
        })
        .collect();
    exec(
        &f.db,
        &format!(
            "INSERT INTO schedule_class_versions (entity_sync_id, reference_value_id, effective_from, \
             cycle_length_days, rotation_start_date, days_json, is_continuous, utilisation_percent, \
             created_at, updated_at) \
             VALUES ('shift-ver-NUIT-2', {night_class}, '2026-04-15', 7, NULL, '{}', 1, 100.0, datetime('now'), datetime('now'))",
            serde_json::Value::Array(days)
        ),
    )
    .await;
    let new_version = scalar_i64(
        &f.db,
        "SELECT id FROM schedule_class_versions WHERE entity_sync_id = 'shift-ver-NUIT-2'",
    )
    .await;
    let wo = work_order(&f.db, Some(f.team)).await;
    let planned = plan(&f.db, &wo, MONDAY, Some(f.night_version), false)
        .await
        .expect("planned on the night shift");
    let moved = plan(&f.db, &planned, "2026-04-16", None, false)
        .await
        .expect("Thursday: the same Classe horaire, its new version");
    assert_eq!(moved.shift_class_version_id, Some(new_version));
    assert_eq!(moved.shift_start.as_deref(), Some("21:00"));
}

/// Before editing a version not yet in force, the editor shows the open work orders planned on it.
#[tokio::test]
async fn editing_a_version_shows_the_work_orders_planned_on_it() {
    let f = setup().await;
    let first = work_order(&f.db, Some(f.team)).await;
    let second = work_order(&f.db, Some(f.team)).await;
    let first = plan(&f.db, &first, MONDAY, Some(f.night_version), false)
        .await
        .expect("planned");
    plan(&f.db, &second, "2026-04-14", Some(f.night_version), false)
        .await
        .expect("planned");
    let impact = crate::personnel::timetable::version_work_order_impact(&f.db, f.night_version)
        .await
        .expect("impact");
    assert_eq!(impact.work_orders, 2);
    assert_eq!(impact.codes[0], first.code, "by planned start");
    let none = crate::personnel::timetable::version_work_order_impact(&f.db, f.other_version)
        .await
        .expect("impact");
    assert_eq!(none.work_orders, 0);
}

// ── AU-111: a reassignment never moves a job into another crew's frozen week ───────────────

/// A planning site (a node whose type can own work) and a crew attached to it. The site's week
/// follows the seeded all-sites weekend (Friday-Saturday): Sunday to Saturday.
async fn planning_crew(db: &DatabaseConnection, code: &str) -> (i64, i64) {
    exec(
        db,
        &format!(
            "INSERT INTO org_node_types (entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, \
             can_carry_cost_center, can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at) \
             SELECT 'au111-type-{code}', structure_model_id, 'AU111{code}', 'Site', 1, 1, 0, 1, 0, 0, 1, \
                    datetime('now'), datetime('now') FROM org_nodes ORDER BY id LIMIT 1"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, \
             created_at, updated_at, row_version, structure_model_id) \
             SELECT 'au111-node-{code}', 'AU111-{code}', 'Site {code}', t.id, NULL, '/', 0, 'active', \
                    datetime('now'), datetime('now'), 1, t.structure_model_id \
               FROM org_node_types t WHERE t.entity_sync_id = 'au111-type-{code}'"
        ),
    )
    .await;
    let site = scalar_i64(
        db,
        &format!("SELECT id FROM org_nodes WHERE entity_sync_id = 'au111-node-{code}'"),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at) \
             VALUES ('au111-team-{code}', 'AU111-CREW-{code}', 'Equipe {code}', 'maintenance', {site}, 'active', \
                     datetime('now'), datetime('now'))"
        ),
    )
    .await;
    let team = scalar_i64(
        db,
        &format!("SELECT id FROM teams WHERE entity_sync_id = 'au111-team-{code}'"),
    )
    .await;
    (site, team)
}

/// `days` days from today (local), `hour`:00 to `hour + 1`:00, as stored (RFC 3339 UTC).
fn local_slot(days: i64, hour: u32) -> (String, String) {
    use chrono::TimeZone;
    let day = chrono::Local::now().date_naive() + chrono::Duration::days(days);
    let at = |h: u32| {
        chrono::Local
            .from_local_datetime(&day.and_hms_opt(h, 0, 0).expect("time"))
            .earliest()
            .expect("local instant")
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };
    (at(hour), at(hour + 1))
}

/// The work order's commitment on `team`, over `slot` (committed, not frozen itself).
async fn commit_wo(db: &DatabaseConnection, wo: &WorkOrder, team: i64, slot: &(String, String)) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO schedule_candidates (source_type, source_id, readiness_status, readiness_score, permit_status, \
             prerequisite_status, estimated_duration_hours, planned_crew_size) \
             VALUES ('work_order', {}, 'ready', 100.0, 'not_required', 'ready', 1.0, 1)",
            wo.id
        ),
    )
    .await;
    let candidate = scalar_i64(
        db,
        &format!(
            "SELECT id FROM schedule_candidates WHERE source_type = 'work_order' AND source_id = {}",
            wo.id
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO schedule_commitments (schedule_candidate_id, source_type, source_id, schedule_period_start, \
             schedule_period_end, committed_start, committed_end, assigned_team_id) \
             VALUES ({candidate}, 'work_order', {wo_id}, substr('{s}', 1, 10), substr('{e}', 1, 10), '{s}', '{e}', {team})",
            wo_id = wo.id,
            s = slot.0,
            e = slot.1
        ),
    )
    .await;
    scalar_i64(
        db,
        &format!(
            "SELECT id FROM schedule_commitments WHERE source_type = 'work_order' AND source_id = {}",
            wo.id
        ),
    )
    .await
}

/// Freezes the week of `site` holding the day `days` from today.
async fn freeze_week(db: &DatabaseConnection, site: i64, days: i64) {
    let day = chrono::Local::now().date_naive() + chrono::Duration::days(days);
    crate::planning::weekly_schedule::freeze_weekly_schedule(
        db,
        admin(db).await,
        crate::planning::weekly_schedule::WeeklyScheduleRef {
            site_id: site,
            day: day.to_string(),
            expected_row_version: None,
        },
    )
    .await
    .expect("week frozen");
}

/// Nothing of the refused reassignment was written: work order, commitment, change log.
async fn assert_untouched(db: &DatabaseConnection, wo: &WorkOrder, commitment: i64, team: i64) {
    let now = reload(db, wo.id).await;
    assert_eq!(now.assigned_team_id, Some(team));
    assert_eq!(now.row_version, wo.row_version);
    let commitment_row = |column: &str| format!("SELECT {column} FROM schedule_commitments WHERE id = {commitment}");
    assert_eq!(scalar_i64(db, &commitment_row("assigned_team_id")).await, team);
    assert_eq!(scalar_i64(db, &commitment_row("row_version")).await, 1);
    assert_eq!(
        scalar_i64(
            db,
            &format!("SELECT COUNT(*) FROM schedule_change_log WHERE commitment_id = {commitment}")
        )
        .await,
        0
    );
}

/// A job on an open site three weeks out, and a crew of a site whose week of that job is frozen.
struct FrozenTarget {
    db: DatabaseConnection,
    wo: WorkOrder,
    commitment: i64,
    open_crew: i64,
    frozen_crew: i64,
}

async fn frozen_target() -> FrozenTarget {
    let f = setup().await;
    let (_, open_crew) = planning_crew(&f.db, "OPEN").await;
    let (frozen_site, frozen_crew) = planning_crew(&f.db, "FROZEN").await;
    let wo = work_order(&f.db, Some(open_crew)).await;
    let commitment = commit_wo(&f.db, &wo, open_crew, &local_slot(21, 8)).await;
    freeze_week(&f.db, frozen_site, 21).await;
    FrozenTarget {
        db: f.db,
        wo,
        commitment,
        open_crew,
        frozen_crew,
    }
}

fn assert_week_frozen(e: &crate::errors::AppError) {
    match e {
        crate::errors::AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues.len(), 1, "{e:?}");
            assert_eq!(issues[0].code, "PLANNING_WEEK_FROZEN", "{e:?}");
            assert!(issues[0].params.contains_key("week_start"), "{e:?}");
        }
        other => panic!("expected PLANNING_WEEK_FROZEN, got {other:?}"),
    }
}

#[tokio::test]
async fn a_reassignment_into_a_crew_whose_site_week_is_frozen_is_refused() {
    let t = frozen_target().await;
    let refused = assign(&t.db, &t.wo, t.frozen_crew)
        .await
        .expect_err("the week is frozen on the new crew's site: a break-in");
    assert_week_frozen(&refused);
    assert_untouched(&t.db, &t.wo, t.commitment, t.open_crew).await;
}

#[tokio::test]
async fn an_emergency_reassignment_never_skips_the_frozen_week() {
    let t = frozen_target().await;
    let refused = reassign(&t.db, &t.wo, t.frozen_crew, None, false)
        .await
        .expect_err("an emergency skips capacity, not the frozen week");
    assert_week_frozen(&refused);
    assert_untouched(&t.db, &t.wo, t.commitment, t.open_crew).await;
}

/// The week's freeze fell due and the automatic freeze has not run on this PC: frozen all the same,
/// as every other PC judges it.
#[tokio::test]
async fn a_week_due_to_freeze_is_refused_like_a_frozen_one() {
    let f = setup().await;
    let (_, open_crew) = planning_crew(&f.db, "OPEN").await;
    let (_, due_crew) = planning_crew(&f.db, "DUE").await;
    let wo = work_order(&f.db, Some(open_crew)).await;
    // The current week: its freeze fell due before it started; nothing froze it here.
    let commitment = commit_wo(&f.db, &wo, open_crew, &local_slot(0, 10)).await;
    let refused = reassign(&f.db, &wo, due_crew, None, false)
        .await
        .expect_err("due to freeze");
    assert_week_frozen(&refused);
    assert_untouched(&f.db, &wo, commitment, open_crew).await;
}

/// The same crew is no move; a crew of a site whose week is open takes the job.
#[tokio::test]
async fn the_same_crew_or_a_crew_on_an_open_week_still_works() {
    let t = frozen_target().await;
    let (_, other_crew) = planning_crew(&t.db, "OTHER").await;
    let same = assign(&t.db, &t.wo, t.open_crew).await.expect("same crew");
    assert_eq!(same.assigned_team_id, Some(t.open_crew));
    let moved = reassign(&t.db, &same, other_crew, None, false)
        .await
        .expect("the other site's week is open");
    assert_eq!(moved.assigned_team_id, Some(other_crew));
    assert_eq!(
        scalar_i64(
            &t.db,
            &format!(
                "SELECT assigned_team_id FROM schedule_commitments WHERE id = {}",
                t.commitment
            )
        )
        .await,
        other_crew
    );
}

/// A commitment frozen itself still refuses any reassignment, with its own code.
#[tokio::test]
async fn a_frozen_commitment_still_refuses_the_reassignment() {
    let f = setup().await;
    let (_, open_crew) = planning_crew(&f.db, "OPEN").await;
    let (_, other_crew) = planning_crew(&f.db, "OTHER").await;
    let wo = work_order(&f.db, Some(open_crew)).await;
    let commitment = commit_wo(&f.db, &wo, open_crew, &local_slot(21, 8)).await;
    exec(
        &f.db,
        &format!("UPDATE schedule_commitments SET frozen_at = '2026-01-01T00:00:00Z' WHERE id = {commitment}"),
    )
    .await;
    for emergency in [false, true] {
        let refused = if emergency {
            reassign(&f.db, &wo, other_crew, None, false).await
        } else {
            assign(&f.db, &wo, other_crew).await
        }
        .expect_err("frozen commitment");
        assert!(
            format!("{refused:?}").contains("GATE_WO_REASSIGN_SCHEDULE_FROZEN"),
            "{refused:?}"
        );
        assert_untouched(&f.db, &wo, commitment, open_crew).await;
    }
}
