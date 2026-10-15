//! Capacity from timetables (Classes horaires): the team's link and its members' inheritance,
//! personal timetables and dated exceptions (another timetable, part-time), breaks and the
//! utilisation factor, rotations (3×8), the site's weekly rest days and holidays (site and parent
//! node), absences, employment dates, dated changes that never rewrite the past, "no schedule"
//! (no default hours) and the upgrade of a database from before timetables.

use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::errors::AppError;
use crate::personnel::calendar::{
    list_holidays, set_site_work_week, upsert_holiday, SiteWorkWeekInput, WorkCalendarHolidayFilter,
    WorkCalendarHolidayUpsertInput,
};
use crate::personnel::capacity::{person_capacity_range, team_capacity_hours, team_capacity_range};
use crate::personnel::timetable::{
    confirm_team_schedule_review, list_class_versions, list_team_schedule, set_team_schedule, upsert_class_version,
    upsert_personnel_schedule_exception, PersonnelScheduleExceptionInput, ScheduleClassVersionInput, ScheduleCycleDay,
    TeamScheduleAssignmentInput,
};

/// Monday 2026-10-12 … Sunday 2026-10-18 (Friday 16 and Saturday 17 are the site's rest days).
fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("date")
}

async fn exec(db: &impl ConnectionTrait, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

fn codes(err: AppError) -> Vec<String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.into_iter().map(|i| i.code).collect(),
        other => panic!("expected coded issues, got {other:?}"),
    }
}

fn line(cycle_day: i64, start: &str, end: &str, break_minutes: i64, rest: bool) -> ScheduleCycleDay {
    ScheduleCycleDay {
        cycle_day,
        shift_start: start.into(),
        shift_end: end.into(),
        break_minutes,
        is_rest_day: rest,
    }
}

/// A Classe horaire with one version in force since 2000 (the service only accepts changes from
/// today on; history is written directly).
async fn class(
    db: &DatabaseConnection,
    code: &str,
    days: &[ScheduleCycleDay],
    rotation_start: Option<&str>,
    utilisation: f64,
    continuous: bool,
) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active) \
             SELECT 'sched-{code}', rs.id, '{code}', 'Classe {code}', 99, 1 \
             FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE UPPER(rd.code) = 'ORG.SCHEDULE_CLASS' AND rs.status = 'published' \
             ORDER BY rs.version_no DESC LIMIT 1"
        ),
    )
    .await;
    let id = scalar_i64(db, &format!("SELECT id FROM reference_values WHERE code = '{code}'")).await;
    let rotation = rotation_start.map_or("NULL".to_string(), |r| format!("'{r}'"));
    let json = serde_json::to_string(days).expect("days");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "INSERT INTO schedule_class_versions (entity_sync_id, reference_value_id, effective_from, cycle_length_days, \
             rotation_start_date, days_json, is_continuous, utilisation_percent, created_at, updated_at) \
             VALUES (?, ?, '2000-01-01', ?, {rotation}, ?, ?, ?, datetime('now'), datetime('now'))"
        ),
        [
            format!("ver-{code}").into(),
            id.into(),
            i64::try_from(days.len()).expect("len").into(),
            json.into(),
            i64::from(continuous).into(),
            utilisation.into(),
        ],
    ))
    .await
    .expect("version");
    id
}

/// A weekly Classe horaire: `start`–`end` with `break_minutes`, rest on `rest` ISO weekdays.
async fn weekly(
    db: &DatabaseConnection,
    code: &str,
    start: &str,
    end: &str,
    break_minutes: i64,
    rest: &[i64],
    utilisation: f64,
) -> i64 {
    let days: Vec<ScheduleCycleDay> = (1..=7)
        .map(|n| line(n, start, end, break_minutes, rest.contains(&n)))
        .collect();
    class(db, code, &days, None, utilisation, false).await
}

async fn link_team(db: &DatabaseConnection, team: i64, class_id: i64, from: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO team_schedule_assignments (entity_sync_id, team_id, schedule_reference_value_id, effective_from, \
             created_at, updated_at) VALUES ('link-{team}-{from}', {team}, {class_id}, '{from}', datetime('now'), \
             datetime('now'))"
        ),
    )
    .await;
}

async fn setup() -> (DatabaseConnection, i64) {
    let db = crate::db::test_db::seeded().await;
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
         VALUES ('cap-empty', 'EQ-VIDE', 'Sans membres', 'maintenance', 'active', datetime('now'), datetime('now'))",
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

async fn admin(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

#[tokio::test]
async fn person_week_follows_timetable_site_rest_days_holiday_and_absence() {
    let (db, team) = setup().await;
    // Monday–Friday timetable on a site resting Friday–Saturday: Friday is lost to the site.
    let day_shift = weekly(&db, "JOUR", "07:30", "15:30", 0, &[6, 7], 100.0).await;
    let p = add_person(&db, "C-1", team, Some(day_shift)).await;
    let actor = admin(&db).await;

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
            label_en: Some("Local holiday".into()),
            entity_id: Some(1),
            is_active: None,
        },
        actor,
    )
    .await
    .expect("holiday");

    let week = person_capacity_range(&db, p, d(12), d(18)).await.expect("week");
    let minutes: Vec<i64> = week.iter().map(|x| x.available_minutes).collect();
    assert_eq!(minutes, vec![480, 240, 480, 0, 0, 0, 0]);
    assert_eq!(week[0].schedule_source.as_deref(), Some("personal"));
    assert_eq!(week[0].plannable_minutes, 480);
    assert_eq!(week[1].absence_types, vec!["training".to_string()]);
    assert_eq!(week[3].reason, "holiday");
    assert_eq!(week[3].holiday_label.as_deref(), Some("Fête locale"));
    assert_eq!(week[4].reason, "site_rest_day");
    assert_eq!(week[5].reason, "rest_day");

    // Same day twice for the same site is refused locally.
    let dup = upsert_holiday(
        &db,
        WorkCalendarHolidayUpsertInput {
            id: None,
            expected_row_version: None,
            holiday_date: "2026-10-15".into(),
            label: "Doublon".into(),
            label_en: None,
            entity_id: Some(1),
            is_active: None,
        },
        actor,
    )
    .await;
    assert!(dup.is_err());
    let holidays = list_holidays(&db, WorkCalendarHolidayFilter::default())
        .await
        .expect("list");
    assert_eq!(holidays.len(), 1);
    assert_eq!(holidays[0].label_en.as_deref(), Some("Local holiday"));
}

#[tokio::test]
async fn night_shift_and_employment_dates() {
    let (db, team) = setup().await;
    let night = weekly(&db, "NUIT", "21:00", "05:00", 0, &[5, 6], 100.0).await;
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
async fn team_members_inherit_its_classe_horaire_with_exceptions_part_time_and_utilisation() {
    let (db, team) = setup().await;
    // The team works the Algerian week, 08:00–16:30 with a 30-minute break, 85 % for work orders.
    let team_class = weekly(&db, "EQ-SEM", "08:00", "16:30", 30, &[5, 6], 85.0).await;
    link_team(&db, team, team_class, "2000-01-01").await;
    let own = weekly(&db, "BUREAU", "08:00", "16:00", 0, &[6, 7], 100.0).await;
    let a = add_person(&db, "C-3", team, None).await;
    let b = add_person(&db, "C-4", team, Some(own)).await;
    let c = add_person(&db, "C-5", team, None).await;
    // b works half time in this team (explicit assignment replaces the primary-team default).
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_team_assignments (personnel_id, team_id, role_code, allocation_percent, is_lead, \
             created_at, updated_at) VALUES ({b}, {team}, 'member', 50, 0, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    // c works part time (50 %) through October.
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_schedule_exceptions (entity_sync_id, personnel_id, work_percent, valid_from, valid_to, \
             created_at, updated_at) VALUES ('exc-c', {c}, 50, '2026-10-01', '2026-10-31', datetime('now'), datetime('now'))"
        ),
    )
    .await;
    // A capacity rule's typed hours do not count; its overtime ceiling does.
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
    assert_eq!(monday.people_scheduled, 3);
    assert_eq!(monday.schedule_class_id, Some(team_class));
    // a: 8 h × 85 % = 6.8; b: own 8 h × 100 % × 50 % allocation = 4; c: 8 h × 50 % × 85 % = 3.4.
    assert!((monday.available_hours - 14.2).abs() < 1e-9, "{monday:?}");
    assert!((monday.attendance_hours - 16.0).abs() < 1e-9, "{monday:?}");
    assert_eq!(monday.basis, "roster");
    assert_eq!(monday.rule_overtime_hours, Some(4.0));
    let member = |id: i64| monday.members.iter().find(|m| m.personnel_id == id).expect("member");
    assert_eq!(member(a).schedule_source.as_deref(), Some("team"));
    assert_eq!(member(b).schedule_source.as_deref(), Some("personal"));
    assert!((member(c).available_hours - 3.4).abs() < 1e-9);

    // Friday: the site rests.
    let friday = team_capacity_hours(&db, team, d(16)).await.expect("friday");
    assert_eq!(friday.available_hours, 0.0);
    assert_eq!(friday.basis, "rest_day");
    // Sunday: a working day of the Algerian week (b's own timetable rests).
    let sunday = team_capacity_hours(&db, team, d(18)).await.expect("sunday");
    assert!((sunday.available_hours - 10.2).abs() < 1e-9, "{sunday:?}");

    let empty_team = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-VIDE'").await;
    let empty = team_capacity_hours(&db, empty_team, d(12)).await.expect("empty");
    assert_eq!(empty.basis, "no_members");
    assert_eq!(empty.available_hours, 0.0);
    // Members but no Classe horaire anywhere: no default hours.
    add_person(&db, "C-6", empty_team, None).await;
    let unscheduled = team_capacity_hours(&db, empty_team, d(12)).await.expect("unscheduled");
    assert_eq!(unscheduled.basis, "no_schedule");
    assert_eq!(unscheduled.available_hours, 0.0);

    assert!(team_capacity_range(&db, team, d(18), d(12)).await.is_err());
}

#[tokio::test]
async fn continuous_rotation_works_through_site_rest_days_and_holidays() {
    let (db, team) = setup().await;
    // 3×8 for one crew: morning, afternoon, night, then a rest day; day 1 on Monday 12 October.
    let rotation = class(
        &db,
        "3X8-A",
        &[
            line(1, "06:00", "14:00", 0, false),
            line(2, "14:00", "22:00", 0, false),
            line(3, "22:00", "06:00", 0, false),
            line(4, "00:00", "00:00", 0, true),
        ],
        Some("2026-10-12"),
        100.0,
        true,
    )
    .await;
    link_team(&db, team, rotation, "2000-01-01").await;
    let p = add_person(&db, "C-7", team, None).await;
    upsert_holiday(
        &db,
        WorkCalendarHolidayUpsertInput {
            id: None,
            expected_row_version: None,
            holiday_date: "2026-10-16".into(),
            label: "Jour férié".into(),
            label_en: None,
            entity_id: None,
            is_active: None,
        },
        admin(&db).await,
    )
    .await
    .expect("holiday");
    let days = person_capacity_range(&db, p, d(12), d(18)).await.expect("days");
    let shifts: Vec<Option<&str>> = days.iter().map(|x| x.shift_start.as_deref()).collect();
    assert_eq!(
        shifts,
        vec![
            Some("06:00"),
            Some("14:00"),
            Some("22:00"),
            None,
            Some("06:00"),
            Some("14:00"),
            Some("22:00")
        ]
    );
    assert_eq!(days[3].reason, "rest_day");
    // Friday 16: site rest day and holiday, yet the continuous crew works.
    assert_eq!(days[4].reason, "working");
    assert_eq!(days[4].available_minutes, 480);
}

#[tokio::test]
async fn timetable_changes_apply_from_a_date_and_never_rewrite_the_past() {
    let (db, team) = setup().await;
    let actor = admin(&db).await;
    let today = Utc::now().date_naive();
    let tomorrow = today + Duration::days(1);
    let all_week: Vec<ScheduleCycleDay> = (1..=7).map(|n| line(n, "08:00", "16:00", 0, false)).collect();
    let team_class = class(&db, "CONT-1", &all_week, None, 100.0, true).await;
    link_team(&db, team, team_class, "2000-01-01").await;
    let p = add_person(&db, "C-8", team, None).await;

    let input = |from: NaiveDate, utilisation: f64| ScheduleClassVersionInput {
        id: None,
        expected_row_version: None,
        reference_value_id: team_class,
        effective_from: from.to_string(),
        cycle_length_days: 7,
        rotation_start_date: None,
        days: all_week.clone(),
        is_continuous: true,
        utilisation_percent: utilisation,
        note: None,
    };
    let past = upsert_class_version(&db, input(today - Duration::days(1), 90.0), actor).await;
    assert_eq!(codes(past.expect_err("past")), vec!["TIMETABLE_DATE_IN_PAST"]);
    let zero = upsert_class_version(&db, input(tomorrow, 0.0), actor).await;
    assert_eq!(codes(zero.expect_err("0 %")), vec!["TIMETABLE_UTILISATION_RANGE"]);
    let mut short = input(tomorrow, 90.0);
    short.days.pop();
    assert_eq!(
        codes(upsert_class_version(&db, short, actor).await.expect_err("6 days")),
        vec!["TIMETABLE_DAYS_INCOMPLETE"]
    );
    let mut long_break = input(tomorrow, 90.0);
    long_break.days[0].break_minutes = 480;
    assert_eq!(
        codes(upsert_class_version(&db, long_break, actor).await.expect_err("break")),
        vec!["TIMETABLE_BREAK_TOO_LONG"]
    );
    let mut rotation = input(tomorrow, 90.0);
    rotation.cycle_length_days = 8;
    rotation.days.push(line(8, "08:00", "16:00", 0, true));
    assert_eq!(
        codes(upsert_class_version(&db, rotation, actor).await.expect_err("no start")),
        vec!["TIMETABLE_ROTATION_START_REQUIRED"]
    );

    let next = upsert_class_version(&db, input(tomorrow, 90.0), actor)
        .await
        .expect("from tomorrow");
    assert_eq!(
        list_class_versions(&db, Some(team_class))
            .await
            .expect("versions")
            .len(),
        2
    );
    let two = person_capacity_range(&db, p, today, tomorrow).await.expect("days");
    assert_eq!(two[0].plannable_minutes, 480, "today keeps the version in force");
    assert_eq!(two[1].plannable_minutes, 432, "tomorrow uses 90 %");
    // The version in force (since 2000) cannot be edited; tomorrow's still can.
    let first_id = list_class_versions(&db, Some(team_class)).await.expect("versions")[0].id;
    let mut edit_first = input(tomorrow + Duration::days(7), 95.0);
    edit_first.id = Some(first_id);
    assert_eq!(
        codes(
            upsert_class_version(&db, edit_first, actor)
                .await
                .expect_err("in force")
        ),
        vec!["TIMETABLE_VERSION_IN_FORCE"]
    );
    let mut edit_next = input(tomorrow, 80.0);
    edit_next.id = Some(next.id);
    assert_eq!(
        upsert_class_version(&db, edit_next, actor)
            .await
            .expect("future edit")
            .utilisation_percent,
        80.0
    );

    // The team moves to another Classe horaire from tomorrow; today is untouched.
    let other = class(&db, "CONT-2", &all_week, None, 50.0, true).await;
    let refused = set_team_schedule(
        &db,
        TeamScheduleAssignmentInput {
            team_id: team,
            schedule_reference_value_id: Some(other),
            effective_from: (today - Duration::days(3)).to_string(),
        },
        actor,
    )
    .await;
    assert_eq!(codes(refused.expect_err("past link")), vec!["TIMETABLE_DATE_IN_PAST"]);
    let links = set_team_schedule(
        &db,
        TeamScheduleAssignmentInput {
            team_id: team,
            schedule_reference_value_id: Some(other),
            effective_from: tomorrow.to_string(),
        },
        actor,
    )
    .await
    .expect("link");
    assert_eq!(links.len(), 2);
    let two = team_capacity_range(&db, team, today, tomorrow).await.expect("team");
    assert_eq!(two[0].schedule_class_id, Some(team_class));
    assert!((two[0].available_hours - 8.0).abs() < 1e-9);
    assert_eq!(two[1].schedule_class_id, Some(other));
    assert!((two[1].available_hours - 4.0).abs() < 1e-9);

    // Exceptions: part time from tomorrow; an overlapping one is refused; a started one can only end.
    let exception = |from: NaiveDate, to: Option<NaiveDate>| PersonnelScheduleExceptionInput {
        id: None,
        expected_row_version: None,
        personnel_id: p,
        schedule_reference_value_id: None,
        work_percent: Some(50.0),
        valid_from: from.to_string(),
        valid_to: to.map(|t| t.to_string()),
        reason: Some("Reprise progressive".into()),
    };
    upsert_personnel_schedule_exception(&db, exception(tomorrow, Some(tomorrow + Duration::days(9))), actor)
        .await
        .expect("part time");
    let overlap = upsert_personnel_schedule_exception(&db, exception(tomorrow + Duration::days(5), None), actor).await;
    assert_eq!(
        codes(overlap.expect_err("overlap")),
        vec!["TIMETABLE_EXCEPTION_OVERLAP"]
    );
    let two = person_capacity_range(&db, p, today, tomorrow).await.expect("days");
    assert_eq!(two[1].work_percent, 50.0);
    assert_eq!(two[1].plannable_minutes, 120, "8 h × 50 % part time × 50 % utilisation");

    // The site's week: from a future date, Sunday becomes a rest day for non-continuous timetables.
    let weeks = set_site_work_week(
        &db,
        SiteWorkWeekInput {
            entity_id: Some(1),
            effective_from: tomorrow.to_string(),
            weekend_days: vec![5, 6, 7],
        },
        actor,
    )
    .await
    .expect("site week");
    assert!(weeks
        .iter()
        .any(|w| w.entity_id == Some(1) && w.weekend_days == vec![5, 6, 7]));
    let past_week = set_site_work_week(
        &db,
        SiteWorkWeekInput {
            entity_id: None,
            effective_from: (today - Duration::days(1)).to_string(),
            weekend_days: vec![5],
        },
        actor,
    )
    .await;
    assert_eq!(codes(past_week.expect_err("past week")), vec!["TIMETABLE_DATE_IN_PAST"]);
}

/// Upgrade path on a database from before timetables: no blanket default. A team that had its own
/// capacity rule keeps that rule's hours as a Classe horaire built from it (its default, flagged
/// for review); other teams get nothing — each member counts their own Classe horaire, and a
/// member without one counts 0 h and is listed as such. Existing Classes horaires get a first
/// version flagged for review.
#[tokio::test]
async fn upgrade_keeps_capacity_rules_as_classes_to_review_and_assumes_no_hours() {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory SQLite");
    exec(&db, "PRAGMA foreign_keys = ON;").await;
    let migrations = crate::migrations::Migrator::migrations();
    let before = migrations
        .iter()
        .position(|m| m.name() == "m20261014_000192_work_calendar_holidays")
        .expect("migration 192 registered");
    crate::migrations::Migrator::up(&db, Some(u32::try_from(before).expect("count")))
        .await
        .expect("migrate to the pre-update schema");
    for (code, name, status) in [
        ("EQ-A", "Mécanique", "active"),
        ("EQ-B", "Électricité", "active"),
        ("EQ-C", "Dissoute", "disbanded"),
        ("EQ-D", "Sans horaires", "active"),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
                 VALUES ('upg-{code}', '{code}', '{name}', 'maintenance', '{status}', datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }
    let team_a = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-A'").await;
    let team_b = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-B'").await;
    let team_c = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-C'").await;
    let team_d = scalar_i64(&db, "SELECT id FROM teams WHERE code = 'EQ-D'").await;
    // EQ-A and EQ-C had their own capacity rule; EQ-B and EQ-D did not.
    for t in [team_a, team_c] {
        exec(
            &db,
            &format!(
                "INSERT INTO capacity_rules (team_id, effective_start, available_hours_per_day, max_overtime_hours_per_day, \
                 row_version, created_at, updated_at) VALUES ({t}, '2026-01-01', 7.5, 2, 1, datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }
    let day_shift = scalar_i64(
        &db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains d ON d.id = rs.domain_id \
         WHERE d.code = 'ORG.SCHEDULE_CLASS' AND rv.code = 'DAY_SHIFT'",
    )
    .await;
    for (code, t, class) in [
        ("UPG-A1", team_a, "NULL".to_string()),
        ("UPG-A2", team_a, "NULL".to_string()),
        ("UPG-B1", team_b, day_shift.to_string()),
        ("UPG-B2", team_b, "NULL".to_string()),
        ("UPG-D1", team_d, "NULL".to_string()),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO personnel (employee_code, full_name, availability_status, primary_team_id, \
                 home_schedule_reference_value_id) VALUES ('{code}', 'Tech {code}', 'available', {t}, {class})"
            ),
        )
        .await;
    }
    // A third EQ-A member, from another team, assigned at 50 %: the rule is shared over
    // 2.5 full-time equivalents, the headcount capacity counts.
    exec(
        &db,
        &format!(
            "INSERT INTO personnel (employee_code, full_name, availability_status, primary_team_id) \
             VALUES ('UPG-A3', 'Tech UPG-A3', 'available', {team_b})"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO personnel_team_assignments (personnel_id, team_id, role_code, allocation_percent, is_lead, \
             created_at, updated_at) SELECT id, {team_a}, 'member', 50, 0, datetime('now'), datetime('now') \
             FROM personnel WHERE employee_code = 'UPG-A3'"
        ),
    )
    .await;

    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("apply the update");

    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) FROM reference_values WHERE code LIKE 'STD_8H%'").await,
        0,
        "no blanket 8-hour Classe horaire"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM schedule_class_versions v JOIN reference_values rv ON rv.id = v.reference_value_id \
             WHERE rv.code = 'DAY_SHIFT' AND v.review_status = 'to_review' AND v.utilisation_percent = 100.0"
        )
        .await,
        1,
        "existing Classes horaires get a first version to review"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM site_work_weeks WHERE entity_id IS NULL AND weekend_days = '5,6'"
        )
        .await,
        1
    );

    // EQ-A: its rule became a Classe horaire, its default, to review.
    let links = list_team_schedule(&db, team_a).await.expect("links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].schedule_code.as_deref(), Some("CAP_RULE_EQ_A"));
    assert_eq!(links[0].review_status.as_deref(), Some("to_review"));
    let rule_class = links[0].schedule_reference_value_id.expect("class");
    let versions = list_class_versions(&db, Some(rule_class)).await.expect("versions");
    assert_eq!(versions.len(), 1);
    let v = &versions[0];
    assert_eq!(v.class_label, "Mécanique — ancienne règle de capacité (7.5 h/jour)");
    assert_eq!(v.utilisation_percent, 100.0);
    assert!(!v.is_continuous, "a day shift on the site's working days");
    assert_eq!(v.working_days_per_cycle, 5);
    assert_eq!(v.review_status.as_deref(), Some("to_review"));
    assert!(
        v.note
            .as_deref()
            .is_some_and(|n| n.contains("7.5 h ÷ 2.5 équivalents temps plein (3 membres") && n.contains("= 3 h")),
        "{:?}",
        v.note
    );
    // The disbanded team and the teams without a rule get no Classe horaire.
    for t in [team_b, team_c, team_d] {
        assert!(list_team_schedule(&db, t).await.expect("links").is_empty());
    }

    // Monday 12 October 2026.
    let monday = d(12);
    let a = team_capacity_hours(&db, team_a, monday).await.expect("team a");
    assert_eq!(a.basis, "roster");
    assert!(
        (a.available_hours - 7.5).abs() < 1e-9,
        "the former team total, shared: {a:?}"
    );
    let b = team_capacity_hours(&db, team_b, monday).await.expect("team b");
    assert!(
        (b.available_hours - 8.0).abs() < 1e-9,
        "only the member with a Classe horaire: {b:?}"
    );
    let without: Vec<&str> = b
        .members
        .iter()
        .filter(|m| m.reason == "no_schedule")
        .map(|m| m.full_name.as_str())
        .collect();
    assert_eq!(
        without,
        vec!["Tech UPG-B2"],
        "a member without a Classe horaire counts 0 h and is listed"
    );
    let d_team = team_capacity_hours(&db, team_d, monday).await.expect("team d");
    assert_eq!(d_team.basis, "no_schedule");
    assert_eq!(d_team.available_hours, 0.0, "never an assumed 8-hour day");
    let hours = crate::planning::scheduling::team_day_hours(&db, team_d, monday)
        .await
        .expect("planning");
    assert_eq!(hours.basis, "no_schedule");

    let reviewed = confirm_team_schedule_review(&db, team_a).await.expect("review");
    assert!(reviewed.iter().all(|l| l.review_status.is_none()));
}
