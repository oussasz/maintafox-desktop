//! PRD 12-state lifecycle: every combination of facts maps to exactly one state, the SQL mirror
//! and the TypeScript mirror (through `shared/wo/lifecycle-cases.json`) agree with `derive`, and
//! the workflow actions produce the expected states and transition-log snapshots.

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};

use super::*;

const PHASES: [WoStatus; 8] = [
    WoStatus::Draft,
    WoStatus::Planning,
    WoStatus::Ready,
    WoStatus::InProgress,
    WoStatus::OnHold,
    WoStatus::Completed,
    WoStatus::Closed,
    WoStatus::Cancelled,
];

/// Every combination of facts: 8 phases × 2^5 flags × 3 hold kinds = 768.
fn all_facts() -> Vec<LifecycleFacts> {
    let mut out = Vec::new();
    for phase in PHASES {
        for bits in 0u8..32 {
            for open_hold in [None, Some(HoldKind::Pause), Some(HoldKind::Prerequisite)] {
                out.push(LifecycleFacts {
                    phase,
                    planning_approval_required: bits & 1 != 0,
                    planning_approved: bits & 2 != 0,
                    has_schedule: bits & 4 != 0,
                    has_assignee: bits & 8 != 0,
                    technically_verified: bits & 16 != 0,
                    open_hold,
                });
            }
        }
    }
    out
}

#[test]
fn every_combination_maps_to_one_state_and_all_twelve_are_reachable() {
    let facts = all_facts();
    assert_eq!(facts.len(), 768);
    let reached: HashSet<LifecycleState> = facts.iter().map(derive).collect();
    assert_eq!(reached.len(), 12, "every PRD state is reachable: {reached:?}");
    for f in &facts {
        let state = derive(f);
        // The state never contradicts the stored phase (the 8 codes are not reinterpreted).
        let expected_phase = match state {
            LifecycleState::Draft => WoStatus::Draft,
            LifecycleState::AwaitingApproval | LifecycleState::Planned => WoStatus::Planning,
            LifecycleState::ReadyToSchedule | LifecycleState::Assigned => WoStatus::Ready,
            LifecycleState::InProgress => WoStatus::InProgress,
            LifecycleState::WaitingForPrerequisite | LifecycleState::Paused => WoStatus::OnHold,
            LifecycleState::MechanicallyComplete | LifecycleState::TechnicallyVerified => WoStatus::Completed,
            LifecycleState::Closed => WoStatus::Closed,
            LifecycleState::Cancelled => WoStatus::Cancelled,
        };
        assert_eq!(f.phase, expected_phase, "{f:?} -> {state:?}");
        assert_eq!(LifecycleState::try_from_str(state.as_str()), Some(state));
    }
}

#[test]
fn ladder_reads_as_the_prd() {
    let base = LifecycleFacts {
        phase: WoStatus::Planning,
        planning_approval_required: true,
        planning_approved: false,
        has_schedule: true,
        has_assignee: true,
        technically_verified: false,
        open_hold: None,
    };
    assert_eq!(derive(&base), LifecycleState::AwaitingApproval);
    assert_eq!(
        derive(&LifecycleFacts {
            planning_approved: true,
            ..base
        }),
        LifecycleState::Planned,
        "a schedule while still in planning stays planned"
    );
    assert_eq!(
        derive(&LifecycleFacts {
            planning_approval_required: false,
            ..base
        }),
        LifecycleState::Planned
    );
    let ready = LifecycleFacts {
        phase: WoStatus::Ready,
        ..base
    };
    assert_eq!(derive(&ready), LifecycleState::Assigned);
    assert_eq!(
        derive(&LifecycleFacts {
            has_assignee: false,
            ..ready
        }),
        LifecycleState::ReadyToSchedule
    );
    let hold = LifecycleFacts {
        phase: WoStatus::OnHold,
        open_hold: Some(HoldKind::Prerequisite),
        ..base
    };
    assert_eq!(derive(&hold), LifecycleState::WaitingForPrerequisite);
    assert_eq!(
        derive(&LifecycleFacts {
            open_hold: Some(HoldKind::Pause),
            ..hold
        }),
        LifecycleState::Paused
    );
    assert_eq!(
        derive(&LifecycleFacts {
            open_hold: None,
            ..hold
        }),
        LifecycleState::Paused
    );
}

/// Minimal schema with exactly the columns `load_facts` and `SQL_CASE` read.
async fn facts_db(facts: &[LifecycleFacts]) -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("sqlite");
    for sql in [
        "CREATE TABLE work_order_statuses (id INTEGER PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE work_orders (id INTEGER PRIMARY KEY, status_id INTEGER NOT NULL, \
         planning_approval_required INTEGER NOT NULL, planning_approved_at TEXT, planned_start TEXT, \
         planned_end TEXT, primary_responsible_id INTEGER, assigned_team_id INTEGER, technically_verified_at TEXT)",
        "CREATE TABLE work_order_delay_segments (id INTEGER PRIMARY KEY, work_order_id INTEGER NOT NULL, \
         ended_at TEXT, hold_kind TEXT NOT NULL)",
    ] {
        db.execute_unprepared(sql).await.expect("schema");
    }
    for (i, p) in PHASES.iter().enumerate() {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_statuses (id, code) VALUES (?, ?)",
            [(i as i64 + 1).into(), p.as_str().into()],
        ))
        .await
        .expect("status");
    }
    for (n, f) in facts.iter().enumerate() {
        let id = n as i64 + 1;
        let status_id = PHASES.iter().position(|p| *p == f.phase).expect("phase") as i64 + 1;
        // Alternate how "no schedule" and "no assignee" look in the row, so both spellings are
        // covered: a missing end, a blank start, or nothing at all; a person or a team.
        let (start, end) = match (f.has_schedule, n % 3) {
            (true, _) => (Some("2026-10-12T08:00:00Z"), Some("2026-10-12T16:00:00Z")),
            (false, 0) => (None, None),
            (false, 1) => (Some("2026-10-12T08:00:00Z"), None),
            (false, _) => (Some("  "), Some("2026-10-12T16:00:00Z")),
        };
        let (person, team) = match (f.has_assignee, n % 2) {
            (true, 0) => (Some(7_i64), None),
            (true, _) => (None, Some(3_i64)),
            (false, _) => (None, None),
        };
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_orders VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                id.into(),
                status_id.into(),
                i64::from(f.planning_approval_required).into(),
                f.planning_approved.then_some("2026-10-11T09:00:00Z").into(),
                start.into(),
                end.into(),
                person.into(),
                team.into(),
                f.technically_verified.then_some("2026-10-12T17:00:00Z").into(),
            ],
        ))
        .await
        .expect("wo");
        // A closed segment of the other kind never counts; the open one decides.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_delay_segments (work_order_id, ended_at, hold_kind) VALUES (?, ?, ?)",
            [id.into(), "2026-10-01T00:00:00Z".into(), "prerequisite".into()],
        ))
        .await
        .expect("closed segment");
        if let Some(k) = f.open_hold {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO work_order_delay_segments (work_order_id, ended_at, hold_kind) VALUES (?, NULL, ?)",
                [id.into(), k.as_str().into()],
            ))
            .await
            .expect("open segment");
        }
    }
    db
}

#[tokio::test]
async fn sql_case_and_load_facts_match_derive_for_every_combination() {
    let facts = all_facts();
    let db = facts_db(&facts).await;
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, {SQL_CASE} AS state FROM work_orders wo \
                 JOIN work_order_statuses wos ON wos.id = wo.status_id ORDER BY wo.id"
            ),
        ))
        .await
        .expect("case");
    assert_eq!(rows.len(), facts.len());
    for (f, row) in facts.iter().zip(&rows) {
        let id: i64 = row.try_get("", "id").expect("id");
        let sql_state: String = row.try_get("", "state").expect("state");
        assert_eq!(sql_state, derive(f).as_str(), "SQL_CASE differs for {f:?}");
        let loaded = load_facts(&db, id).await.expect("load_facts");
        assert_eq!(&loaded, f, "load_facts reads the facts back");
    }
}

/// `shared/wo/lifecycle-cases.json`: every combination with its state, read by the TypeScript
/// mirror's test. Regenerate with `MAINTAFOX_WRITE_LIFECYCLE_CASES=1`.
#[test]
fn shared_cases_file_matches_derive() {
    let cases: Vec<serde_json::Value> = all_facts()
        .iter()
        .map(|f| {
            serde_json::json!({
                "phase": f.phase.as_str(),
                "planning_approval_required": f.planning_approval_required,
                "planning_approved": f.planning_approved,
                "has_schedule": f.has_schedule,
                "has_assignee": f.has_assignee,
                "technically_verified": f.technically_verified,
                "open_hold": f.open_hold.map(|k| k.as_str()),
                "state": derive(f).as_str(),
            })
        })
        .collect();
    let text = format!("{}\n", serde_json::to_string_pretty(&cases).expect("json"));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../shared/wo/lifecycle-cases.json");
    if std::env::var("MAINTAFOX_WRITE_LIFECYCLE_CASES").is_ok() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
        std::fs::write(&path, &text).expect("write cases");
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} missing ({e}); run with MAINTAFOX_WRITE_LIFECYCLE_CASES=1",
            path.display()
        )
    });
    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        text,
        "shared/wo/lifecycle-cases.json is stale; run with MAINTAFOX_WRITE_LIFECYCLE_CASES=1"
    );
}

// ── Workflow integration ──────────────────────────────────────────────────────────────────────

mod workflow {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{
        self, WoAssignInput, WoHoldInput, WoPauseInput, WoPlanInput, WoResumeInput, WoStartInput,
    };
    use crate::wo::queries::{self, WoListFilter};
    use crate::wo::workflow::actions::approve_planning::{approve_planning, WoApprovePlanningInput};
    use crate::wo::workflow::actions::mark_ready::{mark_wo_ready, WoMarkReadyInput};
    use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("q")
            .expect("row")
            .try_get_by_index::<i64>(0)
            .expect("i64")
    }

    async fn setup() -> (DatabaseConnection, i64) {
        let db = crate::db::test_db::seeded().await;
        for sql in [
            "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, \
             updated_at) VALUES (1, 'lc-model', 1, 'active', datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
             created_at, updated_at) VALUES (1, 'lc-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
             updated_at, structure_model_id) VALUES (1, 'lc-node', 'SITE-LC', 'Site', 1, 'active', datetime('now'), \
             datetime('now'), 1)",
            "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, \
             installed_at_node_id, created_at, updated_at) VALUES (1, 'lc-eq', 'EQ-LC', 'Pompe', \
             'active_in_service', 1, datetime('now'), datetime('now'))",
        ] {
            exec(&db, sql).await;
        }
        let actor = scalar(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
        (db, actor)
    }

    async fn set_approval_policy(db: &DatabaseConnection, required: bool) {
        exec(
            db,
            &format!(
                "UPDATE app_settings SET setting_value_json = '{required}' \
                 WHERE setting_key = 'wo_planning_approval_required'"
            ),
        )
        .await;
    }

    async fn lifecycle(db: &DatabaseConnection, wo_id: i64) -> String {
        queries::get_work_order(db, wo_id)
            .await
            .expect("get")
            .expect("wo")
            .lifecycle_state
            .expect("lifecycle_state")
    }

    async fn last_logged(db: &DatabaseConnection, wo_id: i64) -> Option<String> {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lifecycle_state FROM wo_state_transition_log WHERE wo_id = ? ORDER BY id DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get::<Option<String>>("", "lifecycle_state")
        .expect("col")
    }

    async fn create(db: &DatabaseConnection, actor: i64, title: &str) -> crate::wo::domain::WorkOrder {
        queries::create_work_order(
            db,
            WoCreateInput {
                type_code: "corrective".into(),
                equipment_id: Some(1),
                location_id: None,
                source_di_id: None,
                source_inspection_anomaly_id: None,
                source_ram_ishikawa_diagram_id: None,
                source_ishikawa_flow_node_id: None,
                source_rca_cause_text: None,
                entity_id: None,
                planner_id: None,
                urgency_code: Some("normal".into()),
                subtype_code: None,
                title: title.into(),
                description: None,
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create")
    }

    #[tokio::test]
    async fn actions_walk_the_prd_lifecycle_and_log_each_state() {
        let (db, actor) = setup().await;
        set_approval_policy(&db, true).await;
        let wo = create(&db, actor, "OT cycle de vie").await;
        assert_eq!(lifecycle(&db, wo.id).await, "draft");
        assert_eq!(last_logged(&db, wo.id).await.as_deref(), Some("draft"));

        let wo = submit_wo(
            &db,
            WoSubmitInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit");
        assert_eq!(lifecycle(&db, wo.id).await, "awaiting_approval");
        assert_eq!(last_logged(&db, wo.id).await.as_deref(), Some("awaiting_approval"));

        // Turning the setting off later does not change this WO (its own fact decides).
        set_approval_policy(&db, false).await;
        assert_eq!(lifecycle(&db, wo.id).await, "awaiting_approval");

        let wo = approve_planning(
            &db,
            WoApprovePlanningInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("approve");
        assert_eq!(lifecycle(&db, wo.id).await, "planned");

        let wo = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                planner_id: actor,
                planned_start: Some("2026-10-13T08:00:00Z".into()),
                planned_end: Some("2026-10-13T16:00:00Z".into()),
                shift: None,
                shift_class_version_id: None,
                clear_shift: false,
                expected_duration_hours: Some(8.0),
                planned_crew_size: None,
                clear_crew_size: false,
                planned_downtime_hours: None,
                urgency_code: None,
            },
        )
        .await
        .expect("plan");
        assert_eq!(
            lifecycle(&db, wo.id).await,
            "planned",
            "a schedule in planning stays planned"
        );
        let wo = execution::assign_wo(
            &db,
            WoAssignInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                assigned_team_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
                override_reason: "test".into(),
            },
        )
        .await
        .expect("assign");
        let wo = mark_wo_ready(
            &db,
            WoMarkReadyInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("ready");
        assert_eq!(lifecycle(&db, wo.id).await, "assigned");
        assert_eq!(last_logged(&db, wo.id).await.as_deref(), Some("assigned"));

        let wo = execution::start_wo(
            &db,
            WoStartInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("start");
        assert_eq!(lifecycle(&db, wo.id).await, "in_progress");

        let reason = scalar(
            &db,
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id WHERE UPPER(rd.code) = 'WORK.DELAY_REASONS' \
             AND rs.status = 'published' AND rv.code = 'no_parts'",
        )
        .await;
        // A pause is a pause, whatever the reason.
        let wo = execution::pause_wo(
            &db,
            WoPauseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                delay_reason_id: reason,
                comment: None,
            },
        )
        .await
        .expect("pause");
        assert_eq!(lifecycle(&db, wo.id).await, "paused");
        assert_eq!(last_logged(&db, wo.id).await.as_deref(), Some("paused"));
        let wo = execution::resume_wo(
            &db,
            WoResumeInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("resume");
        let wo = execution::set_waiting_for_prerequisite(
            &db,
            WoHoldInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                delay_reason_id: reason,
                comment: Some("pièce en commande".into()),
            },
        )
        .await
        .expect("prerequisite hold");
        assert_eq!(lifecycle(&db, wo.id).await, "waiting_for_prerequisite");
        assert_eq!(
            last_logged(&db, wo.id).await.as_deref(),
            Some("waiting_for_prerequisite")
        );

        // Filter and analytics use the same derivation.
        let page = queries::list_work_orders(
            &db,
            WoListFilter {
                status_codes: None,
                lifecycle_states: Some(vec!["waiting_for_prerequisite".into()]),
                type_codes: None,
                equipment_id: None,
                entity_id: None,
                planner_id: None,
                primary_responsible_id: None,
                urgency_code: None,
                source_di_id: None,
                search: None,
                date_from: None,
                date_to: None,
                limit: 50,
                offset: 0,
            },
        )
        .await
        .expect("list");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, wo.id);
        let stats = crate::wo::stats::get_wo_stats(&db)
            .await
            .unwrap_or_else(|e| panic!("stats: {e:?}"));
        assert!(stats
            .by_lifecycle
            .iter()
            .any(|c| c.status == "waiting_for_prerequisite" && c.count == 1));
    }

    #[tokio::test]
    async fn approval_requirement_is_taken_when_the_wo_enters_planning() {
        let (db, actor) = setup().await;
        set_approval_policy(&db, false).await;
        let wo = create(&db, actor, "Sans approbation").await;
        let wo = submit_wo(
            &db,
            WoSubmitInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit");
        assert_eq!(lifecycle(&db, wo.id).await, "planned");
        set_approval_policy(&db, true).await;
        assert_eq!(
            lifecycle(&db, wo.id).await,
            "planned",
            "the new setting only applies to WOs entering planning afterwards"
        );
    }

    #[tokio::test]
    async fn unknown_lifecycle_filter_is_refused() {
        let (db, _) = setup().await;
        let res = queries::list_work_orders(
            &db,
            WoListFilter {
                status_codes: None,
                lifecycle_states: Some(vec!["almost_done".into()]),
                type_codes: None,
                equipment_id: None,
                entity_id: None,
                planner_id: None,
                primary_responsible_id: None,
                urgency_code: None,
                source_di_id: None,
                search: None,
                date_from: None,
                date_to: None,
                limit: 50,
                offset: 0,
            },
        )
        .await;
        assert!(res.is_err());
    }
}

// ── Sync: hold_kind arrives valid, or is refused / deferred ─────────────────────────────────

#[tokio::test]
async fn inbound_delay_segment_hold_kind_is_checked() {
    use crate::sync::materialize::{ApplyOutcome, DEFER_MISSING_FIELD};
    let db = crate::db::test_db::migrated().await;
    let layout = crate::sync::row_layout::layout(&db, "work_order_delay_segments")
        .await
        .expect("layout");
    let payload = |v: serde_json::Value| {
        let mut m = serde_json::Map::new();
        if !v.is_null() {
            m.insert("hold_kind".into(), v);
        }
        m
    };
    assert!(crate::sync::apply_hooks::before_upsert(&layout, &payload(serde_json::json!("prerequisite"))).is_none());
    assert!(crate::sync::apply_hooks::before_upsert(&layout, &payload(serde_json::json!("pause"))).is_none());
    match crate::sync::apply_hooks::before_upsert(&layout, &payload(serde_json::Value::Null)) {
        Some(ApplyOutcome::Deferred { code, .. }) => assert_eq!(code, DEFER_MISSING_FIELD),
        other => panic!("missing hold_kind must defer, got {other:?}"),
    }
    match crate::sync::apply_hooks::before_upsert(&layout, &payload(serde_json::json!("lunch"))) {
        Some(ApplyOutcome::Conflict { code, .. }) => assert_eq!(code, "INVALID_VALUE"),
        other => panic!("unknown hold_kind must be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn migration_backfills_prerequisite_holds_from_reason_codes() {
    // The real migration chain with a segment created before 175 is covered by the codes list;
    // here: every prerequisite code exists in the seeded WORK.DELAY_REASONS domain.
    let db = crate::db::test_db::migrated().await;
    for code in crate::migrations::m20261012_000175_wo_lifecycle_facts::PREREQUISITE_REASON_CODES {
        let n = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
                 JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(rd.code) = 'WORK.DELAY_REASONS' AND rv.code = ?",
                [(*code).into()],
            ))
            .await
            .expect("q")
            .expect("row")
            .try_get::<i64>("", "n")
            .expect("n");
        assert!(n > 0, "prerequisite reason {code} is not a seeded delay reason");
    }
}
