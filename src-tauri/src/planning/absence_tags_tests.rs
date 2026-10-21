//! Absences on the planning board (decision register 2026-10-19, Q2 and decision (b)): a planner
//! without the sensitive-data right (`per.sensitiveview`, plant-wide) gets a neutral tag only —
//! away, critical for planning or not — decided by the absence type's `critical` behaviour tag in
//! Données de référence; the type, code, label, reason and colour never leave the server for them.
//! Holders of the right keep each absence's type.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::Value;

use super::domain::{AbsenceAccess, PlanningGanttFilter};
use super::scheduling;
use super::tests::{ensure_personnel_with_rate, get_any_team_id, setup_db};
use crate::auth::scope_tests::{assign, role, user};
use crate::personnel::availability::{create_availability_block, AvailabilityBlockCreateInput};
use crate::personnel::reference_lists::ABSENCE_TYPES_DOMAIN;
use crate::rbac::permissions::{PER_MANAGE, PER_SENSITIVEVIEW, PER_VIEW, PLAN_VIEW};
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
async fn lane(db: &DatabaseConnection, team_id: i64, person: i64, access: &AbsenceAccess) -> (String, Vec<Value>) {
    let snapshot = scheduling::get_planning_gantt_snapshot(
        db,
        PlanningGanttFilter {
            period_start: PERIOD_START.to_string(),
            period_end: PERIOD_END.to_string(),
            team_id: Some(team_id),
        },
        access,
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
async fn a_planner_without_the_sensitive_data_right_gets_only_the_neutral_tag() {
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

    let (raw, rows) = lane(&db, team_id, person, &AbsenceAccess::neutral()).await;
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
async fn a_planner_with_the_sensitive_data_right_gets_the_absence_type() {
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

    let (raw, rows) = lane(&db, team_id, person, &AbsenceAccess::full_for_tests()).await;
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

    let (_, rows) = lane(&db, team_id, person, &AbsenceAccess::neutral()).await;
    assert!(!critical(&rows[0]), "tagged leave: plain absence");

    retag(&db, value_id, "critical", actor).await;
    let (raw, rows) = lane(&db, team_id, person, &AbsenceAccess::neutral()).await;
    assert!(
        critical(&rows[0]),
        "tagged critical in Données de référence: critical for planning"
    );
    assert!(!raw.contains("ASTREINTE_ABS") && !raw.contains("#AA0000"));

    retag(&db, value_id, "training", actor).await;
    let (_, rows) = lane(&db, team_id, person, &AbsenceAccess::neutral()).await;
    assert!(!critical(&rows[0]), "tagged training again: plain absence");
}

/// A user holding `roles`, each assigned plant-wide (`tenant`) or on one site.
async fn account(db: &DatabaseConnection, name: &str, permissions: &[&str], site: Option<i64>) -> i32 {
    let id = user(db, name).await;
    let role_id = role(db, &format!("{name}-role"), permissions).await;
    match site {
        None => assign(db, id, role_id, "tenant", None).await,
        Some(node) => assign(db, id, role_id, "org_node", Some(node)).await,
    }
    id
}

#[tokio::test]
async fn the_board_shows_the_type_only_with_the_sensitive_data_right() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let (team_id, person, actor) = board_person(&db, "TECH-ABS-TAG-4").await;
    absence(
        &db,
        person,
        actor,
        "medical",
        "2026-09-02T08:00:00Z",
        "2026-09-02T16:00:00Z",
    )
    .await;

    // The personnel view alone (a planner reading the personnel screens): neutral tag only.
    let planner = account(&db, "abs-tag-planner", &[PLAN_VIEW, PER_VIEW], None).await;
    let access = AbsenceAccess::load(&db, &cache, planner).await;
    let (raw, rows) = lane(&db, team_id, person, &access).await;
    assert_eq!(keys(&rows[0]), ["critical", "end_at", "start_at"]);
    assert!(critical(&rows[0]));
    assert!(!raw.contains("medical"), "{raw}");

    // With the sensitive-data right plant-wide: the type.
    let manager = account(&db, "abs-tag-manager", &[PLAN_VIEW, PER_VIEW, PER_SENSITIVEVIEW], None).await;
    let access = AbsenceAccess::load(&db, &cache, manager).await;
    let (_, rows) = lane(&db, team_id, person, &access).await;
    assert_eq!(rows[0]["block_type"], "medical");
}

#[tokio::test]
async fn the_sensitive_data_right_on_one_site_only_still_gives_the_neutral_tag() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let sites = crate::auth::scope_tests::tree(&db).await;
    let (team_id, person, actor) = board_person(&db, "TECH-ABS-TAG-5").await;
    absence(
        &db,
        person,
        actor,
        "leave",
        "2026-09-01T08:00:00Z",
        "2026-09-01T16:00:00Z",
    )
    .await;

    let site_planner = account(
        &db,
        "abs-tag-site",
        &[PLAN_VIEW, PER_VIEW, PER_SENSITIVEVIEW],
        Some(sites.site_a),
    )
    .await;
    let access = AbsenceAccess::load(&db, &cache, site_planner).await;
    let (raw, rows) = lane(&db, team_id, person, &access).await;
    assert_eq!(keys(&rows[0]), ["critical", "end_at", "start_at"]);
    assert!(!raw.contains("leave"), "{raw}");
}

/// The availability screens (block list, calendar, availability state) and the person's capacity
/// days: per.view alone gets no type; the sensitive-data right, or the person's own record, does.
#[tokio::test]
async fn every_absence_screen_shows_the_type_only_with_the_sensitive_data_right() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let (_, person, actor) = board_person(&db, "TECH-ABS-SCREENS").await;
    absence(
        &db,
        person,
        actor,
        "medical",
        "2026-09-02T00:00:00Z",
        "2026-09-03T00:00:00Z",
    )
    .await;
    // A second person away right now, for the availability state.
    let (_, away, _) = board_person(&db, "TECH-ABS-NOW").await;
    absence(
        &db,
        away,
        actor,
        "leave",
        "2000-01-01T00:00:00Z",
        "2100-01-01T00:00:00Z",
    )
    .await;

    let viewer = account(&db, "abs-scr-viewer", &[PER_VIEW], None).await;
    let sensitive = account(&db, "abs-scr-sensitive", &[PER_VIEW, PER_SENSITIVEVIEW], None).await;
    // A user linked to the person's record, without the right: their own absences stay readable.
    let own = account(&db, "abs-scr-own", &[PER_VIEW], None).await;
    exec(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = {person} WHERE id = {own}"),
    )
    .await;

    let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 2).expect("day");
    for (user_id, full_for_person) in [(viewer, false), (sensitive, true), (own, true)] {
        let access = AbsenceAccess::load(&db, &cache, user_id).await;

        // Availability block list.
        let mut blocks = crate::personnel::queries::list_personnel_availability_blocks(&db, person, 50)
            .await
            .expect("blocks");
        access.apply_to_blocks(&db, &mut blocks).await.expect("apply");
        let payload = serde_json::to_value(&blocks).expect("json");
        let block = &payload[0];
        assert_eq!(block["critical_for_planning"], true, "medical is tagged critical");
        if full_for_person {
            assert_eq!(block["block_type"], "medical", "user {user_id}");
            assert_eq!(block["reason_note"], SECRET_REASON);
            assert_eq!(block["type_masked"], false);
        } else {
            assert!(block.get("block_type").is_none(), "no type in the payload: {block}");
            assert!(block["reason_note"].is_null());
            assert_eq!(block["type_masked"], true);
            assert!(!payload.to_string().contains("medical"));
        }

        // Availability calendar.
        let mut entries = crate::personnel::availability::list_availability_calendar(
            &db,
            crate::personnel::availability::AvailabilityCalendarFilter {
                date_from: "2026-09-02".to_string(),
                date_to: "2026-09-02".to_string(),
                personnel_id: Some(person),
                entity_id: None,
                team_id: None,
                include_inactive: Some(true),
            },
        )
        .await
        .expect("calendar");
        access.apply_to_calendar(&mut entries);
        let types: Vec<String> = entries.iter().flat_map(|e| e.block_types.clone()).collect();
        assert_eq!(
            types.is_empty(),
            !full_for_person,
            "calendar types for user {user_id}: {types:?}"
        );

        // The person's capacity days.
        let mut days = crate::personnel::capacity::person_capacity_range(&db, person, day, day)
            .await
            .expect("capacity");
        access.apply_to_person_capacity(person, &mut days);
        let types: Vec<String> = days.iter().flat_map(|d| d.absence_types.clone()).collect();
        assert_eq!(
            types.is_empty(),
            !full_for_person,
            "capacity types for user {user_id}: {types:?}"
        );
        assert!(days.iter().any(|d| d.absent_minutes > 0), "the absence still counts");

        // The availability state of the person away now (not the own-record user's record).
        let mut state = crate::personnel::availability::compute_personnel_availability_state(&db, away)
            .await
            .expect("state");
        access.apply_to_availability_state(&mut state);
        let sees_type = user_id == sensitive;
        assert_eq!(
            state.reasons.contains(&"leave".to_string()),
            sees_type,
            "{:?}",
            state.reasons
        );
    }
}

async fn critical_audits(db: &DatabaseConnection) -> Vec<sea_orm::QueryResult> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT actor_id, target_id, details_json FROM audit_events WHERE action_code = '{}' ORDER BY id",
            crate::personnel::availability::AUDIT_BLOCK_CRITICAL_CHANGED
        ),
    ))
    .await
    .expect("audit rows")
}

/// Setting or clearing the critical flag of a block is audited: who, block, person, before and
/// after — never the type or reason. An edit that keeps the type and reason hidden keeps them.
#[tokio::test]
async fn changing_a_blocks_critical_flag_is_audited_without_type_or_reason() {
    let db = setup_db().await;
    let (_, person, actor) = board_person(&db, "TECH-ABS-AUDIT").await;
    absence(
        &db,
        person,
        actor,
        "leave",
        "2026-09-05T08:00:00Z",
        "2026-09-05T16:00:00Z",
    )
    .await;
    let block_id = scalar(
        &db,
        &format!("SELECT id FROM personnel_availability_blocks WHERE personnel_id = {person}"),
    )
    .await;
    let edit = |is_critical: bool| crate::personnel::availability::AvailabilityBlockUpdateInput {
        id: block_id,
        // A caller who may not read the type and reason keeps them.
        block_type: None,
        start_at: "2026-09-05T08:00:00Z".to_string(),
        end_at: "2026-09-05T16:00:00Z".to_string(),
        reason_note: None,
        keep_reason_note: true,
        is_critical: Some(is_critical),
        confirm_overlap: None,
    };

    // Same flag: nothing to audit.
    crate::personnel::availability::update_availability_block(&db, edit(false), actor, &AbsenceAccess::neutral())
        .await
        .expect("edit");
    assert!(critical_audits(&db).await.is_empty());

    let block =
        crate::personnel::availability::update_availability_block(&db, edit(true), actor, &AbsenceAccess::neutral())
            .await
            .expect("set critical");
    assert!(block.is_critical);
    assert_eq!(block.block_type, "leave", "the hidden type is kept");
    assert_eq!(
        block.reason_note.as_deref(),
        Some(SECRET_REASON),
        "the hidden reason is kept"
    );
    crate::personnel::availability::update_availability_block(&db, edit(false), actor, &AbsenceAccess::neutral())
        .await
        .expect("clear critical");

    let rows = critical_audits(&db).await;
    assert_eq!(rows.len(), 2);
    let mut details: Vec<Value> = Vec::new();
    for r in &rows {
        assert_eq!(r.try_get::<Option<i64>>("", "actor_id").expect("actor"), Some(actor));
        assert_eq!(
            r.try_get::<Option<String>>("", "target_id").expect("target"),
            Some(block_id.to_string())
        );
        let raw: String = r.try_get("", "details_json").expect("details");
        assert!(!raw.contains("leave") && !raw.contains(SECRET_REASON), "{raw}");
        details.push(serde_json::from_str(&raw).expect("json"));
    }
    assert_eq!(details[0]["personnel_id"], person);
    assert_eq!(details[0]["block_id"], block_id);
    assert_eq!(
        (
            details[0]["is_critical_before"].as_bool(),
            details[0]["is_critical_after"].as_bool()
        ),
        (Some(false), Some(true))
    );
    assert_eq!(
        (
            details[1]["is_critical_before"].as_bool(),
            details[1]["is_critical_after"].as_bool()
        ),
        (Some(true), Some(false))
    );
}

/// The server never takes hidden fields from the payload: a per.manage user without the
/// sensitive-data right who calls the backend directly cannot change the type (refused, coded,
/// even a right guess) nor erase the reason (kept); dates and the critical flag stay editable.
#[tokio::test]
async fn a_forged_edit_cannot_change_a_hidden_type_or_erase_a_hidden_reason() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let (_, person, actor) = board_person(&db, "TECH-ABS-FORGED").await;
    absence(
        &db,
        person,
        actor,
        "medical",
        "2026-09-06T08:00:00Z",
        "2026-09-06T16:00:00Z",
    )
    .await;
    let block_id = scalar(
        &db,
        &format!("SELECT id FROM personnel_availability_blocks WHERE personnel_id = {person}"),
    )
    .await;
    let manager = account(&db, "abs-forged-manager", &[PER_VIEW, PER_MANAGE], None).await;
    let access = AbsenceAccess::load(&db, &cache, manager).await;
    let forged = |block_type: Option<&str>| crate::personnel::availability::AvailabilityBlockUpdateInput {
        id: block_id,
        block_type: block_type.map(str::to_string),
        start_at: "2026-09-06T09:00:00Z".to_string(),
        end_at: "2026-09-06T16:00:00Z".to_string(),
        // An empty reason, without asking to keep it.
        reason_note: None,
        keep_reason_note: false,
        is_critical: None,
        confirm_overlap: None,
    };

    for guess in ["training", "medical"] {
        let err = crate::personnel::availability::update_availability_block(
            &db,
            forged(Some(guess)),
            manager.into(),
            &access,
        )
        .await
        .expect_err("a type sent by a caller who cannot read it is refused");
        match err {
            crate::errors::AppError::OrgValidationFailed(issues) => {
                assert_eq!(issues[0].code, crate::personnel::availability::CODE_ABSENCE_TYPE_HIDDEN);
            }
            other => panic!("expected a coded refusal, got {other:?}"),
        }
    }

    let block = crate::personnel::availability::update_availability_block(&db, forged(None), manager.into(), &access)
        .await
        .expect("dates stay editable");
    assert_eq!(block.start_at, "2026-09-06T09:00:00Z");
    assert_eq!(block.block_type, "medical", "the stored type is kept");
    assert_eq!(
        block.reason_note.as_deref(),
        Some(SECRET_REASON),
        "the stored reason is kept"
    );
}

/// Without the sensitive-data right the availability state keeps its category (on leave) but no
/// type is sent (CMMS Advisor: keep the status category).
#[tokio::test]
async fn the_availability_state_keeps_the_category_but_sends_no_type() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let (_, away, actor) = board_person(&db, "TECH-ABS-CATEGORY").await;
    absence(
        &db,
        away,
        actor,
        "leave",
        "2000-01-01T00:00:00Z",
        "2100-01-01T00:00:00Z",
    )
    .await;
    let planner = account(&db, "abs-category-planner", &[PLAN_VIEW, PER_VIEW], None).await;
    let access = AbsenceAccess::load(&db, &cache, planner).await;

    let mut state = crate::personnel::availability::compute_personnel_availability_state(&db, away)
        .await
        .expect("state");
    access.apply_to_availability_state(&mut state);
    assert_eq!(state.status, "on_leave", "the category stays");
    assert!(state.reasons.is_empty(), "no absence type: {:?}", state.reasons);
    let payload = serde_json::to_value(&state).expect("json");
    assert_eq!(payload["reasons"], serde_json::json!([]));
}

/// On one board, a planner without the sensitive-data right sees the type on their own lane (the
/// personnel record linked to their account) while every other lane stays neutral.
#[tokio::test]
async fn a_planner_sees_the_type_on_their_own_lane_only() {
    let db = setup_db().await;
    let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());
    let (team_id, own_person, actor) = board_person(&db, "TECH-ABS-OWN").await;
    let (_, colleague, _) = board_person(&db, "TECH-ABS-COLLEAGUE").await;
    absence(
        &db,
        own_person,
        actor,
        "medical",
        "2026-09-02T08:00:00Z",
        "2026-09-02T16:00:00Z",
    )
    .await;
    absence(
        &db,
        colleague,
        actor,
        "medical",
        "2026-09-03T08:00:00Z",
        "2026-09-03T16:00:00Z",
    )
    .await;
    let planner = account(&db, "abs-own-planner", &[PLAN_VIEW, PER_VIEW], None).await;
    exec(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = {own_person} WHERE id = {planner}"),
    )
    .await;
    let access = AbsenceAccess::load(&db, &cache, planner).await;

    let snapshot = scheduling::get_planning_gantt_snapshot(
        &db,
        PlanningGanttFilter {
            period_start: PERIOD_START.to_string(),
            period_end: PERIOD_END.to_string(),
            team_id: Some(team_id),
        },
        &access,
    )
    .await
    .expect("snapshot");
    let lane_of = |id: i64| -> Vec<Value> {
        let lane = snapshot
            .assignee_lanes
            .iter()
            .find(|l| l.personnel_id == id)
            .expect("lane");
        serde_json::from_str(&lane.blocked_intervals_json).expect("json")
    };
    let own = lane_of(own_person);
    assert_eq!(own[0]["block_type"], "medical", "their own absence keeps its type");
    let other = lane_of(colleague);
    assert_eq!(
        keys(&other[0]),
        ["critical", "end_at", "start_at"],
        "a colleague's stays neutral"
    );
    for lane in &snapshot.assignee_lanes {
        if lane.personnel_id != own_person {
            assert!(
                !lane.blocked_intervals_json.contains("block_type"),
                "{}",
                lane.blocked_intervals_json
            );
        }
    }
}
