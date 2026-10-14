//! Capacity from shift calendars: day / night shifts, rest days, holidays (site and parent
//! node), absences, employment dates, team allocation and "no schedule" (no default hours).

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::personnel::calendar::{list_holidays, upsert_holiday, WorkCalendarHolidayFilter, WorkCalendarHolidayUpsertInput};
use crate::personnel::capacity::{person_capacity_range, team_capacity_hours, team_capacity_range};

/// Monday 2026-10-12 … Sunday 2026-10-18.
fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("date")
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
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

/// A schedule (ORG.SCHEDULE_CLASS value) with Mon–Fri shifts `start`–`end`, weekend rest.
async fn schedule(db: &DatabaseConnection, code: &str, start: &str, end: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active) \
             SELECT 'sched-{code}', rs.id, '{code}', '{code}', 99, 1 FROM reference_sets rs \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE UPPER(rd.code) = 'ORG.SCHEDULE_CLASS' AND rs.status = 'published' \
             ORDER BY rs.version_no DESC LIMIT 1"
        ),
    )
    .await;
    let id = scalar_i64(db, &format!("SELECT id FROM reference_values WHERE code = '{code}'")).await;
    for dow in 1..=7 {
        let rest = i64::from(dow >= 6);
        exec(
            db,
            &format!(
                "INSERT INTO schedule_details (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) \
                 VALUES ({id}, {dow}, '{start}', '{end}', {rest})"
            ),
        )
        .await;
    }
    id
}

async fn setup() -> (DatabaseConnection, i64) {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory SQLite");
    exec(&db, "PRAGMA foreign_keys = ON;").await;
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db).await.expect("seeder");
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'cap-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'cap-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, \
         structure_model_id) VALUES (1, 'cap-site', 'SITE', 'Usine', 1, 'active', datetime('now'), datetime('now'), 1)",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, parent_id, status, created_at, \
         updated_at, structure_model_id) VALUES (2, 'cap-shop', 'ATELIER', 'Atelier', 1, 1, 'active', datetime('now'), \
         datetime('now'), 1)",
        "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
         VALUES ('cap-team', 'EQ-MEC', 'Mécanique', 'maintenance', 'active', datetime('now'), datetime('now'))",
        "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
         VALUES ('cap-empty', 'EQ-VIDE', 'Sans planning', 'maintenance', 'active', datetime('now'), datetime('now'))",
    ] {
        exec(&db, sql).await;
    }
    let team = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-MEC'").await;
    (db, team)
}

async fn add_person(db: &DatabaseConnection, code: &str, team: i64, schedule_id: Option<i64>) -> i64 {
    let schedule = schedule_id.map_or("NULL".to_string(), |s| s.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO personnel (employee_code, full_name, availability_status, primary_team_id, primary_entity_id, \
             home_schedule_reference_value_id) VALUES ('{code}', 'Tech {code}', 'available', {team}, 2, {schedule})"
        ),
    )
    .await;
    scalar_i64(db, &format!("SELECT id FROM personnel WHERE employee_code = '{code}'")).await
}

#[tokio::test]
async fn person_day_shift_rest_days_absence_and_holiday() {
    let (db, team) = setup().await;
    let day_shift = schedule(&db, "JOUR", "07:30", "15:30").await;
    let p = add_person(&db, "C-1", team, Some(day_shift)).await;
    let actor = scalar_i64(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;

    // Half-day absence on Tuesday, holiday on Thursday for the parent site (applies to the shop).
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_availability_blocks (personnel_id, block_type, start_at, end_at, is_critical, created_at) \
             VALUES ({p}, 'training', '2026-10-13T07:30:00Z', '2026-10-13T11:30:00Z', 0, datetime('now'))"
        ),
    )
    .await;
    upsert_holiday(
        &db,
        WorkCalendarHolidayUpsertInput {
            id: None,
            expected_row_version: None,
            holiday_date: "2026-10-15".into(),
            label: "Fête locale".into(),
            entity_id: Some(1),
            is_active: None,
        },
        actor,
    )
    .await
    .expect("holiday");

    let week = person_capacity_range(&db, p, d(12), d(18)).await.expect("week");
    let minutes: Vec<i64> = week.iter().map(|x| x.available_minutes).collect();
    assert_eq!(minutes, vec![480, 240, 480, 0, 480, 0, 0]);
    assert_eq!(week[1].absence_types, vec!["training".to_string()]);
    assert_eq!(week[3].reason, "holiday");
    assert_eq!(week[3].holiday_label.as_deref(), Some("Fête locale"));
    assert_eq!(week[5].reason, "rest_day");

    // Same day twice for the same site is refused locally.
    let dup = upsert_holiday(
        &db,
        WorkCalendarHolidayUpsertInput {
            id: None,
            expected_row_version: None,
            holiday_date: "2026-10-15".into(),
            label: "Doublon".into(),
            entity_id: Some(1),
            is_active: None,
        },
        actor,
    )
    .await;
    assert!(dup.is_err());
    assert_eq!(
        list_holidays(&db, WorkCalendarHolidayFilter::default())
            .await
            .expect("list")
            .len(),
        1
    );
}

#[tokio::test]
async fn night_shift_and_employment_dates() {
    let (db, team) = setup().await;
    let night = schedule(&db, "NUIT", "21:00", "05:00").await;
    let p = add_person(&db, "C-2", team, Some(night)).await;
    exec(
        &db,
        &format!("UPDATE personnel SET contract_end_date = '2026-10-14' WHERE id = {p}"),
    )
    .await;
    // Absence after midnight inside Monday's night shift.
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_availability_blocks (personnel_id, block_type, start_at, end_at, is_critical, created_at) \
             VALUES ({p}, 'medical', '2026-10-13T03:00:00Z', '2026-10-13T05:00:00Z', 0, datetime('now'))"
        ),
    )
    .await;
    let days = person_capacity_range(&db, p, d(12), d(15)).await.expect("days");
    assert_eq!(days[0].scheduled_minutes, 480);
    assert_eq!(days[0].available_minutes, 360);
    assert_eq!(days[1].available_minutes, 480);
    assert_eq!(days[3].reason, "not_employed");
}

#[tokio::test]
async fn team_capacity_sums_members_with_allocation() {
    let (db, team) = setup().await;
    let day_shift = schedule(&db, "JOUR2", "08:00", "16:00").await;
    let a = add_person(&db, "C-3", team, Some(day_shift)).await;
    let b = add_person(&db, "C-4", team, Some(day_shift)).await;
    let _no_schedule = add_person(&db, "C-5", team, None).await;
    // b works half time in this team (explicit assignment replaces the primary-team default).
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_team_assignments (personnel_id, team_id, role_code, allocation_percent, is_lead, \
             created_at, updated_at) VALUES ({b}, {team}, 'member', 50, 0, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO capacity_rules (team_id, effective_start, available_hours_per_day, max_overtime_hours_per_day, \
             row_version, created_at, updated_at) VALUES ({team}, '2026-01-01', 20, 4, 1, datetime('now'), datetime('now'))"
        ),
    )
    .await;

    let monday = team_capacity_hours(&db, team, d(12)).await.expect("monday");
    assert_eq!(monday.people, 3);
    assert_eq!(monday.people_scheduled, 2);
    assert!((monday.available_hours - 12.0).abs() < 1e-9, "{monday:?}");
    assert_eq!(monday.basis, "roster");
    assert_eq!(monday.rule_available_hours, Some(20.0));
    assert_eq!(monday.rule_overtime_hours, Some(4.0));
    assert!(monday.members.iter().any(|m| m.personnel_id == a && (m.available_hours - 8.0).abs() < 1e-9));

    let sunday = team_capacity_hours(&db, team, d(18)).await.expect("sunday");
    assert_eq!(sunday.available_hours, 0.0);
    assert_eq!(sunday.basis, "no_schedule");

    let empty_team = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-VIDE'").await;
    let empty = team_capacity_hours(&db, empty_team, d(12)).await.expect("empty");
    assert_eq!(empty.basis, "no_members");
    assert_eq!(empty.available_hours, 0.0);

    assert!(team_capacity_range(&db, team, d(18), d(12)).await.is_err());
}
