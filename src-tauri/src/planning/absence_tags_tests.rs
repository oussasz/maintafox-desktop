//! Absences on the planning board (decision register 2026-10-19, Q2): a planner without `per.view`
//! gets a neutral tag only — away, critical for planning or not — decided by the absence type's
//! `critical` behaviour tag in Données de référence; the type, code, label, reason and colour never
//! leave the server for them. Holders of `per.view` keep each absence's type.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::Value;

use super::domain::{AbsenceDetail, PlanningGanttFilter};
use super::scheduling;
use super::tests::{ensure_personnel_with_rate, get_any_team_id, setup_db};
use crate::auth::scope_tests::{assign, role, user};
use crate::personnel::availability::{create_availability_block, AvailabilityBlockCreateInput};
use crate::personnel::reference_lists::ABSENCE_TYPES_DOMAIN;
use crate::rbac::permissions::{PER_VIEW, PLAN_VIEW};
use crate::reference::values::{update_value, UpdateReferenceValuePayload};

const PERIOD_START: &str = "2026-09-01T00:00:00Z";
const PERIOD_END: &str = "2026-09-08T00:00:00Z";
const SECRET_REASON: &str = "Hospital appointment";

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("value")
}

/// A crew member on the board (a roster lane) and the admin who records their absences.
async fn board_person(db: &DatabaseConnection, code: &str) -> (i64, i64, i64) {
    let team_id = get_any_team_id(db).await;
    let person = ensure_personnel_with_rate(db, code).await;
    exec(
        db,
        &format!(
            "INSERT INTO personnel_team_assignments (personnel_id, team_id, role_code, allocation_percent, \
                valid_from, valid_to, is_lead, created_at, updated_at) \
             VALUES ({person}, {team_id}, 'member', 100.0, '2026-01-01T00:00:00Z', NULL, 0, \
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let actor = scalar(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    (team_id, person, actor)
}

async fn absence(db: &DatabaseConnection, person: i64, actor: i64, code: &str, start: &str, end: &str) {
    create_availability_block(
        db,
        AvailabilityBlockCreateInput {
            personnel_id: person,
            block_type: code.to_string(),
            start_at: start.to_string(),
            end_at: end.to_string(),
            reason_note: Some(SECRET_REASON.to_string()),
            is_critical: None,
            confirm_overlap: None,
        },
        actor,
    )
    .await
    .unwrap_or_else(|e| panic!("absence {code}: {e:?}"));
}

/// The person's lane on the board, as the given caller receives it: the raw JSON and its rows.
async fn lane(db: &DatabaseConnection, team_id: i64, person: i64, detail: AbsenceDetail) -> (String, Vec<Value>) {
    let snapshot = scheduling::get_planning_gantt_snapshot(
        db,
        PlanningGanttFilter {
            period_start: PERIOD_START.to_string(),
            period_end: PERIOD_END.to_string(),
            team_id: Some(team_id),
        },
        detail,
    )
    .await
    .expect("snapshot");
    let lane = snapshot
        .assignee_lanes
        .into_iter()
        .find(|l| l.personnel_id == person)
        .expect("the person's lane");
    let rows: Vec<Value> = serde_json::from_str(&lane.blocked_intervals_json).expect("intervals json");
    (lane.blocked_intervals_json, rows)
}

fn keys(row: &Value) -> Vec<String> {
    let mut keys: Vec<String> = row.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

fn critical(row: &Value) -> bool {
    row["critical"].as_bool().expect("critical flag")
}

/// A value added to the plant's absence-type list with its behaviour tag (as an admin adds it).
async fn plant_absence_type(db: &DatabaseConnection, code: &str, tag: &str) -> i64 {
    exec(
        db,
        &format!(
            "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, semantic_tag, is_active, color_hex) \
             SELECT 'abs-tag-{code}', rs.id, '{code}', 'Label {code}', 99, '{tag}', 1, '#AA0000' \
               FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE d.code = '{ABSENCE_TYPES_DOMAIN}' AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1"
        ),
    )
    .await;
    scalar(
        db,
        &format!("SELECT id FROM reference_values WHERE entity_sync_id = 'abs-tag-{code}'"),
    )
    .await
}

/// An admin changes the behaviour tag of the type in Données de référence (the editor's path).
async fn retag(db: &DatabaseConnection, value_id: i64, tag: &str, actor: i64) {
    update_value(
        db,
        value_id,
        UpdateReferenceValuePayload {
            label: None,
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: Some(Some(tag.to_string())),
            external_code: None,
            metadata_json: None,
        },
        actor,
    )
    .await
    .expect("retag the absence type");
}

#[tokio::test]
async fn a_planner_without_personnel_view_gets_only_the_neutral_tag() {
    let db = setup_db().await;
    let (team_id, person, actor) = board_person(&db, "TECH-ABS-TAG-1").await;
    // Seeded list: `leave` is tagged leave, `medical` critical, `training` training.
    absence(
        &db,
        person,
        actor,
        "leave",
        "2026-09-01T08:00:00Z",
        "2026-09-01T16:00:00Z",
    )
    .await;
    absence(
        &db,
        person,
        actor,
        "medical",
        "2026-09-02T08:00:00Z",
        "2026-09-02T16:00:00Z",
    )
    .await;
    absence(
        &db,
        person,
        actor,
        "training",
        "2026-09-03T08:00:00Z",
        "2026-09-03T16:00:00Z",
    )
    .await;

    let (raw, rows) = lane(&db, team_id, person, AbsenceDetail::AvailabilityOnly).await;
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(
            keys(row),
            ["critical", "end_at", "start_at"],
            "only when, and the tag: {row}"
        );
    }
    assert_eq!(
        rows.iter().map(critical).collect::<Vec<_>>(),
        [false, true, false],
        "critical for planning comes from the type's tag in Données de référence"
    );
    for leaked in [
        "leave",
        "medical",
        "training",
        "Congé",
        "Medical",
        "Formation",
        SECRET_REASON,
        "color",
    ] {
        assert!(!raw.contains(leaked), "the board must not carry '{leaked}': {raw}");
    }
}

#[tokio::test]
async fn a_planner_with_personnel_view_gets_the_absence_type() {
    let db = setup_db().await;
    let (team_id, person, actor) = board_person(&db, "TECH-ABS-TAG-2").await;
    absence(
        &db,
        person,
        actor,
        "leave",
        "2026-09-01T08:00:00Z",
        "2026-09-01T16:00:00Z",
    )
    .await;
    absence(
        &db,
        person,
        actor,
        "medical",
        "2026-09-02T08:00:00Z",
        "2026-09-02T16:00:00Z",
    )
    .await;

    let (raw, rows) = lane(&db, team_id, person, AbsenceDetail::Full).await;
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(keys(row), ["block_type", "critical", "end_at", "start_at"]);
    }
    assert_eq!(rows[0]["block_type"], "leave");
    assert!(!critical(&rows[0]));
    assert_eq!(rows[1]["block_type"], "medical");
    assert!(critical(&rows[1]), "a type tagged critical makes its block critical");
    // The reason stays in the personnel screens (per.sensitiveview), never on the board.
    assert!(!raw.contains(SECRET_REASON));
}

#[tokio::test]
async fn changing_the_types_tag_in_reference_data_changes_the_neutral_tag() {
    let db = setup_db().await;
    let (team_id, person, actor) = board_person(&db, "TECH-ABS-TAG-3").await;
    let value_id = plant_absence_type(&db, "ASTREINTE_ABS", "leave").await;
    absence(
        &db,
        person,
        actor,
        "ASTREINTE_ABS",
        "2026-09-04T08:00:00Z",
        "2026-09-04T16:00:00Z",
    )
    .await;

    let (_, rows) = lane(&db, team_id, person, AbsenceDetail::AvailabilityOnly).await;
    assert!(!critical(&rows[0]), "tagged leave: plain absence");

    retag(&db, value_id, "critical", actor).await;
    let (raw, rows) = lane(&db, team_id, person, AbsenceDetail::AvailabilityOnly).await;
    assert!(
        critical(&rows[0]),
        "tagged critical in Données de référence: critical for planning"
    );
    assert!(!raw.contains("ASTREINTE_ABS") && !raw.contains("#AA0000"));

    retag(&db, value_id, "training", actor).await;
    let (_, rows) = lane(&db, team_id, person, AbsenceDetail::AvailabilityOnly).await;
    assert!(!critical(&rows[0]), "tagged training again: plain absence");
}

#[tokio::test]
async fn the_board_detail_follows_the_personnel_view_right() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let planner = user(&db, "abs-tag-planner").await;
    let planning_role = role(&db, "abs-tag-planning", &[PLAN_VIEW]).await;
    assign(&db, planner, planning_role, "tenant", None).await;
    assert_eq!(
        scheduling::absence_detail_for(&db, &cache, planner)
            .await
            .expect("detail"),
        AbsenceDetail::AvailabilityOnly
    );

    let manager = user(&db, "abs-tag-manager").await;
    let personnel_role = role(&db, "abs-tag-personnel", &[PLAN_VIEW, PER_VIEW]).await;
    assign(&db, manager, personnel_role, "tenant", None).await;
    assert_eq!(
        scheduling::absence_detail_for(&db, &cache, manager)
            .await
            .expect("detail"),
        AbsenceDetail::Full
    );
}
