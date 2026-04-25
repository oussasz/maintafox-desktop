//! SP07 observability cross-module integration tests (roadmap 04).

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

use crate::activity::emitter::{
    emit_activity_event, emit_di_event, emit_wo_event, ActivityEventInput,
};
use crate::archive::integrity::verify_checksum;
use crate::archive::writer::{archive_record, ArchiveInput};
use crate::audit::writer::{write_audit_event, AuditEventInput};
use crate::auth::session_manager::AuthenticatedUser;
use crate::commands::activity_feed::{build_event_chain, EventChainInput};
use crate::commands::admin_users::{assign_role_scope_impl, AssignRoleScopeInput};
use crate::commands::archive::evaluate_purge_eligibility_db;
use crate::di::conversion::{convert_di_to_work_order, WoConversionInput};
use crate::di::queries::{create_intervention_request, DiCreateInput};
use crate::di::review::{approve_di_for_planning, screen_di, DiApproveInput, DiScreenInput};
use crate::notifications::delivery;
use crate::notifications::emitter::{emit_event, NotificationEventInput};
use crate::notifications::scheduler;
use crate::state::AppState;
use crate::wo::closeout::{
    self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput,
};
use crate::wo::domain::WoCreateInput;
use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPlanInput};
use crate::wo::labor::{self, AddLaborInput};
use crate::wo::parts::{self, AddPartInput};
use crate::wo::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should connect");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA foreign_keys = ON;".to_string(),
    ))
    .await
    .expect("PRAGMA foreign_keys");

    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations should apply cleanly");

    crate::db::seeder::seed_system_data(&db)
        .await
        .expect("seeder should run cleanly");

    db
}

async fn admin_id(db: &DatabaseConnection) -> i64 {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1".to_string(),
        ))
        .await
        .expect("query")
        .expect("admin user must exist");
    row.try_get::<i64>("", "id").expect("decode admin id")
}

async fn create_user(db: &DatabaseConnection, username: &str) -> i64 {
    let now = chrono::Utc::now().to_rfc3339();
    let sync_id = format!("{username}-sync");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts \
         (sync_id, username, display_name, identity_mode, password_hash, \
          is_active, is_admin, force_password_change, failed_login_attempts, \
          created_at, updated_at, row_version) \
         VALUES (?, ?, ?, 'local', 'no-login-needed', 1, 0, 0, 0, ?, ?, 1)",
        [
            sync_id.into(),
            username.to_string().into(),
            format!("{username} display").into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .expect("insert user");

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = ? LIMIT 1",
            [username.to_string().into()],
        ))
        .await
        .expect("query")
        .expect("user");
    row.try_get::<i64>("", "id").expect("id")
}

async fn create_verifier(db: &DatabaseConnection, username: &str) -> i64 {
    create_user(db, username).await
}

async fn seed_di_fk_data(db: &DatabaseConnection) {
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO equipment \
         (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
         VALUES (1, 'obs-eq-001', 'EQ-OBS-001', 'Obs Equipment', 'active_in_service', \
                 datetime('now'), datetime('now'));".to_string(),
    ))
    .await
    .expect("equipment");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_structure_models \
         (id, sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'obs-model-001', 1, 'active', datetime('now'), datetime('now'));".to_string(),
    ))
    .await
    .expect("structure model");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_node_types \
         (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
         VALUES (1, 'obs-nt-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));".to_string(),
    ))
    .await
    .expect("node type");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_nodes \
         (id, sync_id, code, name, node_type_id, status, created_at, updated_at) \
         VALUES (1, 'obs-on-001', 'SITE-OBS', 'Obs Site', 1, 'active', \
                 datetime('now'), datetime('now'));".to_string(),
    ))
    .await
    .expect("org_nodes");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_domains \
         (id, code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
         VALUES (1, 'DI_CLASSIFICATION', 'DI Classification', 'flat', 'tenant_managed', 1, \
                 datetime('now'), datetime('now'));".to_string(),
    ))
    .await
    .expect("reference_domains");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_sets \
         (id, domain_id, version_no, status, created_at) \
         VALUES (1, 1, 1, 'published', datetime('now'));".to_string(),
    ))
    .await
    .expect("reference_sets");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (id, set_id, code, label, is_active) \
         VALUES (1, 1, 'MECH', 'Mécanique', 1);".to_string(),
    ))
    .await
    .expect("reference_values");
}

fn di_create_input(submitter_id: i64) -> DiCreateInput {
    DiCreateInput {
        asset_id: 1,
        org_node_id: 1,
        title: "Obs DI".to_string(),
        description: "Observability chain".to_string(),
        origin_type: "operator".to_string(),
        symptom_code_id: None,
        impact_level: "unknown".to_string(),
        production_impact: false,
        safety_flag: false,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: "medium".to_string(),
        observed_at: None,
        source_inspection_anomaly_id: None,
        submitter_id,
    }
}

async fn advance_di_to_approved(db: &DatabaseConnection, user_id: i64) -> (i64, i64) {
    let di = create_intervention_request(db, di_create_input(user_id))
        .await
        .expect("create DI");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE intervention_requests SET status = 'pending_review', \
         row_version = row_version + 1, updated_at = datetime('now') WHERE id = ?",
        [di.id.into()],
    ))
    .await
    .expect("pending_review");

    let screened = screen_di(
        db,
        DiScreenInput {
            di_id: di.id,
            actor_id: user_id,
            expected_row_version: 2,
            validated_urgency: "high".to_string(),
            review_team_id: Some(1),
            classification_code_id: Some(1),
            reviewer_note: Some("OK".to_string()),
        },
    )
    .await
    .expect("screen");

    let approved = approve_di_for_planning(
        db,
        DiApproveInput {
            di_id: di.id,
            actor_id: user_id,
            expected_row_version: screened.row_version,
            notes: Some("approved".to_string()),
        },
    )
    .await
    .expect("approve");

    (di.id, approved.row_version)
}

async fn transition_planned_assigned_in_progress_for_user(
    db: &DatabaseConnection,
    wo_id: i64,
    start_rv: i64,
    actor: i64,
    primary: i64,
) -> crate::wo::domain::WorkOrder {
    let wo = execution::plan_wo(
        db,
        WoPlanInput {
            wo_id,
            actor_id: actor,
            expected_row_version: start_rv,
            planner_id: actor,
            planned_start: "2026-04-10T08:00:00Z".into(),
            planned_end: "2026-04-10T16:00:00Z".into(),
            shift: None,
            expected_duration_hours: Some(8.0),
            urgency_id: None,
        },
    )
    .await
    .expect("plan_wo");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET \
         status_id = (SELECT id FROM work_order_statuses WHERE code = 'ready_to_schedule'), \
         row_version = row_version + 1, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("ready_to_schedule");

    let wo = execution::assign_wo(
        db,
        WoAssignInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo.row_version + 1,
            assigned_group_id: None,
            primary_responsible_id: Some(primary),
            scheduled_at: None,
        },
    )
    .await
    .expect("assign_wo");

    execution::start_wo(
        db,
        execution::WoStartInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("start_wo")
}

async fn close_wo_all_gates(db: &DatabaseConnection, actor: i64) -> crate::wo::domain::WorkOrder {
    let verifier = create_verifier(db, "obsverifier").await;
    let wo = queries::create_work_order(
        db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: None,
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "Obs close".into(),
            description: Some("test".into()),
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(8.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("create wo");

    let wo = transition_planned_assigned_in_progress_for_user(db, wo.id, wo.row_version, actor, actor).await;

    labor::add_labor_entry(
        db,
        AddLaborInput {
            wo_id: wo.id,
            intervener_id: actor,
            skill_id: None,
            started_at: Some("2026-04-10T08:00:00Z".into()),
            ended_at: Some("2026-04-10T10:00:00Z".into()),
            hours_worked: None,
            hourly_rate: Some(60.0),
            notes: None,
        },
    )
    .await
    .expect("labor");

    let part = parts::add_planned_part(
        db,
        AddPartInput {
            wo_id: wo.id,
            article_id: None,
            article_ref: Some("OBS-P1".into()),
            quantity_planned: 2.0,
            unit_cost: Some(45.0),
            stock_location_id: None,
            auto_reserve: Some(false),
            notes: None,
        },
    )
    .await
    .expect("part");

    parts::record_actual_usage(db, part.id, 2.0, Some(45.0))
        .await
        .expect("usage");

    let wo = execution::complete_wo_mechanically(
        db,
        WoMechCompleteInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: Some("done".into()),
        },
    )
    .await
    .expect("mech complete");

    closeout::save_failure_detail(
        db,
        SaveFailureDetailInput {
            wo_id: wo.id,
            symptom_id: None,
            failure_mode_id: None,
            failure_cause_id: None,
            failure_effect_id: None,
            is_temporary_repair: false,
            is_permanent_repair: true,
            cause_not_determined: true,
            notes: Some("captured".into()),
        },
    )
    .await
    .expect("failure");

    closeout::update_wo_rca(
        db,
        UpdateWoRcaInput {
            wo_id: wo.id,
            root_cause_summary: Some("rca".into()),
            corrective_action_summary: Some("fix".into()),
        },
    )
    .await
    .expect("rca");

    let (_v, wo) = closeout::save_verification(
        db,
        SaveVerificationInput {
            wo_id: wo.id,
            verified_by_id: verifier,
            result: "pass".into(),
            return_to_service_confirmed: true,
            recurrence_risk_level: Some("low".into()),
            notes: None,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("verification");

    closeout::close_wo(
        db,
        WoCloseInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
            ..Default::default()
        },
    )
    .await
    .expect("close")
}

async fn role_id_by_name(db: &DatabaseConnection, name: &str) -> i64 {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE name = ? AND deleted_at IS NULL LIMIT 1",
            [name.into()],
        ))
        .await
        .expect("q")
        .expect("role");
    row.try_get("", "id").expect("id")
}

async fn insert_event_link(db: &DatabaseConnection, parent_id: i64, child_id: i64) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO event_links \
         (parent_event_id, child_event_id, parent_table, child_table, link_type) \
         VALUES (?, ?, 'activity_events', 'activity_events', 'correlation')",
        [parent_id.into(), child_id.into()],
    ))
    .await
    .expect("event_links");
}

#[tokio::test]
async fn test_obs_01_emit_creates_notification() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let assignee = create_user(&db, "obs_assignee_01").await;

    let wo = queries::create_work_order(
        &db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: None,
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "N1".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(1.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("wo");

    let wo = transition_planned_assigned_in_progress_for_user(&db, wo.id, wo.row_version, actor, assignee).await;

    emit_event(
        &db,
        NotificationEventInput {
            source_module: "wo".into(),
            source_record_id: Some(wo.id.to_string()),
            event_code: "wo.assigned".into(),
            category_code: "wo_assigned".into(),
            severity: "info".into(),
            dedupe_key: None,
            payload_json: None,
            title: "Assigned".into(),
            body: Some("body".into()),
            action_url: None,
        },
    )
    .await
    .expect("emit");

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT delivery_state FROM notifications WHERE recipient_user_id = ? LIMIT 1",
            [assignee.into()],
        ))
        .await
        .expect("q")
        .expect("notif");

    let state: String = row.try_get("", "delivery_state").expect("delivery_state");
    assert_eq!(state, "delivered");
}

#[tokio::test]
async fn test_obs_02_dedupe_prevents_flood() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let assignee = create_user(&db, "obs_assignee_02").await;
    let wo = queries::create_work_order(
        &db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: None,
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "N2".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(1.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("wo");
    let wo = transition_planned_assigned_in_progress_for_user(&db, wo.id, wo.row_version, actor, assignee).await;

    let dk = "obs-dedupe-xyz";
    for (title, body) in [("t1", "b1"), ("t2", "b2")] {
        emit_event(
            &db,
            NotificationEventInput {
                source_module: "wo".into(),
                source_record_id: Some(wo.id.to_string()),
                event_code: "wo.assigned".into(),
                category_code: "wo_assigned".into(),
                severity: "info".into(),
                dedupe_key: Some(dk.into()),
                payload_json: None,
                title: title.into(),
                body: Some(body.into()),
                action_url: None,
            },
        )
        .await
        .expect("emit");
    }

    let cnt: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM notifications n \
             JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.dedupe_key = ?",
            [dk.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert_eq!(cnt, 1);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT title, body FROM notifications n \
             JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.dedupe_key = ? LIMIT 1",
            [dk.into()],
        ))
        .await
        .expect("q")
        .expect("row");
    assert_eq!(row.try_get::<String>("", "title").unwrap(), "t2");
    assert_eq!(row.try_get::<String>("", "body").unwrap(), "b2");
}

#[tokio::test]
async fn test_obs_03_snooze_wakes() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let u = create_user(&db, "obs_snooze").await;
    let wo = queries::create_work_order(
        &db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: None,
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "N3".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(1.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("wo");
    let wo = transition_planned_assigned_in_progress_for_user(&db, wo.id, wo.row_version, actor, u).await;

    emit_event(
        &db,
        NotificationEventInput {
            source_module: "wo".into(),
            source_record_id: Some(wo.id.to_string()),
            event_code: "wo.assigned".into(),
            category_code: "wo_assigned".into(),
            severity: "info".into(),
            dedupe_key: None,
            payload_json: None,
            title: "Snooze".into(),
            body: None,
            action_url: None,
        },
    )
    .await
    .expect("emit");

    let nid: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM notifications WHERE recipient_user_id = ? LIMIT 1",
            [u.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    delivery::snooze(&db, nid, u, 1).await.expect("snooze");

    let st: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT delivery_state FROM notifications WHERE id = ?",
            [nid.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "delivery_state")
        .expect("st");
    assert_eq!(st, "snoozed");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications SET snoozed_until = strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-5 minutes')) WHERE id = ?",
        [nid.into()],
    ))
    .await
    .expect("backdate snooze");

    scheduler::run_scheduler_tick_for_test(&db)
        .await
        .expect("tick");

    let st2: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT delivery_state FROM notifications WHERE id = ?",
            [nid.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "delivery_state")
        .expect("st");
    assert_eq!(st2, "delivered");
}

#[tokio::test]
async fn test_obs_04_acknowledge_closes_escalation_path() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let u = create_user(&db, "obs_ack").await;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE notification_rules SET requires_ack = 1 WHERE category_code = 'wo_assigned'".to_string(),
    ))
    .await
    .expect("rule");

    let wo = queries::create_work_order(
        &db,
        WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: None,
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: Some(3),
            title: "N4".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: Some(1.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("wo");
    let wo = transition_planned_assigned_in_progress_for_user(&db, wo.id, wo.row_version, actor, u).await;

    emit_event(
        &db,
        NotificationEventInput {
            source_module: "wo".into(),
            source_record_id: Some(wo.id.to_string()),
            event_code: "wo.assigned".into(),
            category_code: "wo_assigned".into(),
            severity: "info".into(),
            dedupe_key: None,
            payload_json: None,
            title: "Ack".into(),
            body: None,
            action_url: None,
        },
    )
    .await
    .expect("emit");

    let nid: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM notifications WHERE recipient_user_id = ? LIMIT 1",
            [u.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    delivery::acknowledge(&db, nid, u, None)
        .await
        .expect("acknowledge_notification equivalent (delivery layer)");

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT delivery_state, escalation_level FROM notifications WHERE id = ?",
            [nid.into()],
        ))
        .await
        .expect("q")
        .expect("r");
    assert_eq!(row.try_get::<String>("", "delivery_state").unwrap(), "acknowledged");
    assert_eq!(row.try_get::<i64>("", "escalation_level").unwrap(), 0);
}

#[tokio::test]
async fn test_obs_05_archive_wo_and_verify() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let wo = close_wo_all_gates(&db, actor).await;

    let aid = archive_record(
        &db,
        ArchiveInput {
            source_module: "wo".into(),
            source_record_id: wo.id.to_string(),
            archive_class: "operational_history".into(),
            source_state: Some("closed".into()),
            archive_reason_code: "completed".into(),
            archived_by_id: Some(actor),
            restore_policy: "not_allowed".into(),
            restore_until_at: None,
            payload_json: serde_json::json!({ "wo_id": wo.id }),
            workflow_history_json: None,
            attachment_manifest_json: None,
            config_version_refs_json: None,
            search_text: Some("obs".into()),
        },
    )
    .await
    .expect("archive");

    let item_cnt: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM archive_items WHERE id = ?",
            [aid.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert_eq!(item_cnt, 1);

    let psz: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT payload_size_bytes FROM archive_payloads WHERE archive_item_id = ?",
            [aid.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "payload_size_bytes")
        .expect("psz");
    assert!(psz > 0);

    let ok = verify_checksum(&db, aid).await.expect("verify");
    assert!(ok);

    let actions = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT action FROM archive_actions WHERE archive_item_id = ? ORDER BY id ASC",
            [aid.into()],
        ))
        .await
        .expect("q");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].try_get::<String>("", "action").unwrap(), "archive");
    assert_eq!(
        actions[1].try_get::<String>("", "action").unwrap(),
        "checksum_verified"
    );
}

#[tokio::test]
async fn test_obs_06_purge_blocked_by_retention() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let wo = close_wo_all_gates(&db, actor).await;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE retention_policies SET allow_purge = 1 \
         WHERE module_code = 'wo' AND archive_class = 'operational_history'"
            .to_string(),
    ))
    .await
    .expect("allow_purge");

    let aid = archive_record(
        &db,
        ArchiveInput {
            source_module: "wo".into(),
            source_record_id: wo.id.to_string(),
            archive_class: "operational_history".into(),
            source_state: Some("closed".into()),
            archive_reason_code: "completed".into(),
            archived_by_id: Some(actor),
            restore_policy: "not_allowed".into(),
            restore_until_at: None,
            payload_json: serde_json::json!({ "wo_id": wo.id }),
            workflow_history_json: None,
            attachment_manifest_json: None,
            config_version_refs_json: None,
            search_text: None,
        },
    )
    .await
    .expect("archive");

    let yesterday = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(1))
        .expect("yesterday")
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE archive_items SET archived_at = ? WHERE id = ?",
        [yesterday.into(), aid.into()],
    ))
    .await
    .expect("archived_at");

    let reason = evaluate_purge_eligibility_db(&db, aid)
        .await
        .expect("eval")
        .expect("blocked");
    assert!(
        reason.contains("retention") || reason.contains("elapsed"),
        "unexpected reason: {reason}"
    );
}

#[tokio::test]
async fn test_obs_07_legal_hold_blocks_purge() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let wo = close_wo_all_gates(&db, actor).await;

    let aid = archive_record(
        &db,
        ArchiveInput {
            source_module: "wo".into(),
            source_record_id: wo.id.to_string(),
            archive_class: "operational_history".into(),
            source_state: Some("closed".into()),
            archive_reason_code: "completed".into(),
            archived_by_id: Some(actor),
            restore_policy: "not_allowed".into(),
            restore_until_at: None,
            payload_json: serde_json::json!({ "wo_id": wo.id }),
            workflow_history_json: None,
            attachment_manifest_json: None,
            config_version_refs_json: None,
            search_text: None,
        },
    )
    .await
    .expect("archive");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE archive_items SET legal_hold = 1 WHERE id = ?",
        [aid.into()],
    ))
    .await
    .expect("legal hold flag");

    let reason = evaluate_purge_eligibility_db(&db, aid)
        .await
        .expect("eval")
        .expect("blocked");
    assert!(
        reason.to_lowercase().contains("legal"),
        "purge eligibility should cite legal hold: {reason}"
    );
}

#[tokio::test]
async fn test_obs_08_restore_blocked_for_operational_history() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let wo = close_wo_all_gates(&db, actor).await;

    let aid = archive_record(
        &db,
        ArchiveInput {
            source_module: "wo".into(),
            source_record_id: wo.id.to_string(),
            archive_class: "operational_history".into(),
            source_state: Some("closed".into()),
            archive_reason_code: "completed".into(),
            archived_by_id: Some(actor),
            restore_policy: "not_allowed".into(),
            restore_until_at: None,
            payload_json: serde_json::json!({ "wo_id": wo.id }),
            workflow_history_json: None,
            attachment_manifest_json: None,
            config_version_refs_json: None,
            search_text: None,
        },
    )
    .await
    .expect("archive");

    let pol: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT restore_policy FROM archive_items WHERE id = ?",
            [aid.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "restore_policy")
        .expect("restore_policy");
    assert_eq!(pol, "not_allowed");
    assert!(
        !matches!(pol.as_str(), "admin_only" | "until_date"),
        "restore_archive_item rejects this policy with PermissionDenied (same guard as IPC)"
    );
}

#[tokio::test]
async fn test_obs_09_wo_close_emits_activity_event() {
    let db = setup_db().await;
    let actor = admin_id(&db).await;
    let wo = close_wo_all_gates(&db, actor).await;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM activity_events \
             WHERE event_code = 'wo.closed' AND source_module = 'wo' AND source_record_id = ? \
             LIMIT 1",
            [wo.id.to_string().into()],
        ))
        .await
        .expect("q");
    assert!(row.is_some(), "expected wo.closed activity row");
}

#[tokio::test]
async fn test_obs_10_rbac_mutation_emits_both_activity_and_audit() {
    let db = setup_db().await;
    let target = create_user(&db, "obs_rbac_target").await;
    let readonly_rid = role_id_by_name(&db, "Readonly").await;

    let admin = admin_id(&db).await;
    let caller = AuthenticatedUser {
        user_id: i32::try_from(admin).expect("admin id fits i32"),
        username: "admin".into(),
        display_name: None,
        is_admin: true,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    };
    let state = AppState::new(db.clone());
    assign_role_scope_impl(
        &state,
        &caller,
        AssignRoleScopeInput {
            user_id: target,
            role_id: readonly_rid,
            scope_type: "tenant".into(),
            scope_reference: None,
            valid_from: None,
            valid_to: None,
        },
        false,
        None,
    )
    .await
    .expect("assign_role_scope_impl matches assign_role_scope DB + ledger path");

    let ace: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM admin_change_events WHERE action = 'role_assigned'",
            [],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert_eq!(ace, 1);

    let aud: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM audit_events WHERE action_code = ?",
            [crate::audit::event_type::ROLE_ASSIGNED.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert_eq!(aud, 1);

    let act: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM activity_events WHERE event_code = 'rbac.role_assigned'",
            [],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert_eq!(act, 1);
}

#[tokio::test]
async fn test_obs_11_audit_append_only() {
    let db = setup_db().await;

    let id = write_audit_event(
        &db,
        AuditEventInput {
            action_code: "test.obs_audit".into(),
            target_type: Some("case".into()),
            target_id: Some("42".into()),
            actor_id: Some(1),
            auth_context: "password".into(),
            result: "success".into(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".into(),
            details_json: Some(serde_json::json!({"k": 1})),
        },
    )
    .await
    .expect("insert");

    let orig: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT result FROM audit_events WHERE id = ?",
            [id.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "result")
        .expect("result");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE audit_events SET result = 'altered' WHERE id = ?",
        [id.into()],
    ))
    .await
    .expect("tamper");

    let now: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT result FROM audit_events WHERE id = ?",
            [id.into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "result")
        .expect("result");
    assert_eq!(now, "altered");
    assert_ne!(now, orig);

    let lib = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"));
    assert!(
        !lib.contains("update_audit_event") && !lib.contains("delete_audit_event"),
        "lib.rs must not register mutating audit IPC aliases"
    );
}

#[tokio::test]
async fn test_obs_12_full_observability_chain() {
    let db = setup_db().await;
    seed_di_fk_data(&db).await;
    let actor = admin_id(&db).await;
    let corr = Uuid::new_v4().to_string();

    let _ = emit_activity_event(
        &db,
        ActivityEventInput {
            event_class: "operational".into(),
            event_code: "iot.threshold_exceeded".into(),
            source_module: "iot".into(),
            source_record_type: Some("sensor".into()),
            source_record_id: Some("S-1".into()),
            entity_scope_id: None,
            actor_id: Some(actor),
            severity: "warn".into(),
            summary_json: None,
            correlation_id: Some(corr.clone()),
            visibility_scope: "global".into(),
        },
    )
    .await;

    let iot_id: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM activity_events WHERE event_code = 'iot.threshold_exceeded' ORDER BY id DESC LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    let (di_id, approved_rv) = advance_di_to_approved(&db, actor).await;

    let _ = emit_di_event(
        &db,
        di_id,
        "di.submitted",
        Some(actor),
        None,
        Some(corr.clone()),
    )
    .await;

    let di_ev: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM activity_events WHERE event_code = 'di.submitted' AND source_record_id = ? ORDER BY id DESC LIMIT 1",
            [di_id.to_string().into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    let conv = convert_di_to_work_order(
        &db,
        WoConversionInput {
            di_id,
            actor_id: actor,
            expected_row_version: approved_rv,
            conversion_notes: None,
        },
    )
    .await
    .expect("convert");

    let _ = emit_wo_event(
        &db,
        conv.wo_id,
        "wo.created",
        Some(actor),
        None,
        Some(corr.clone()),
    )
    .await;

    let wo_ev: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM activity_events WHERE event_code = 'wo.created' AND source_record_id = ? ORDER BY id DESC LIMIT 1",
            [conv.wo_id.to_string().into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    let wo_work = transition_planned_assigned_in_progress_for_user(
        &db,
        conv.wo_id,
        1,
        actor,
        actor,
    )
    .await;

    labor::add_labor_entry(
        &db,
        AddLaborInput {
            wo_id: wo_work.id,
            intervener_id: actor,
            skill_id: None,
            started_at: Some("2026-04-10T08:00:00Z".into()),
            ended_at: Some("2026-04-10T10:00:00Z".into()),
            hours_worked: None,
            hourly_rate: Some(60.0),
            notes: None,
        },
    )
    .await
    .expect("labor");

    let part = parts::add_planned_part(
        &db,
        AddPartInput {
            wo_id: wo_work.id,
            article_id: None,
            article_ref: Some("OBS12".into()),
            quantity_planned: 2.0,
            unit_cost: Some(45.0),
            stock_location_id: None,
            auto_reserve: Some(false),
            notes: None,
        },
    )
    .await
    .expect("part");

    parts::record_actual_usage(&db, part.id, 2.0, Some(45.0))
        .await
        .expect("usage");

    let wo_work = execution::complete_wo_mechanically(
        &db,
        WoMechCompleteInput {
            wo_id: wo_work.id,
            actor_id: actor,
            expected_row_version: wo_work.row_version,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: Some("done".into()),
        },
    )
    .await
    .expect("mech");

    let verifier = create_verifier(&db, "obs12ver").await;
    closeout::save_failure_detail(
        &db,
        SaveFailureDetailInput {
            wo_id: wo_work.id,
            symptom_id: None,
            failure_mode_id: None,
            failure_cause_id: None,
            failure_effect_id: None,
            is_temporary_repair: false,
            is_permanent_repair: true,
            cause_not_determined: true,
            notes: Some("n".into()),
        },
    )
    .await
    .expect("fail");

    closeout::update_wo_rca(
        &db,
        UpdateWoRcaInput {
            wo_id: wo_work.id,
            root_cause_summary: Some("r".into()),
            corrective_action_summary: Some("c".into()),
        },
    )
    .await
    .expect("rca");

    let (_v, wo_work) = closeout::save_verification(
        &db,
        SaveVerificationInput {
            wo_id: wo_work.id,
            verified_by_id: verifier,
            result: "pass".into(),
            return_to_service_confirmed: true,
            recurrence_risk_level: Some("low".into()),
            notes: None,
            expected_row_version: wo_work.row_version,
        },
    )
    .await
    .expect("ver");

    let closed = closeout::close_wo(
        &db,
        WoCloseInput {
            wo_id: wo_work.id,
            actor_id: actor,
            expected_row_version: wo_work.row_version,
            ..Default::default()
        },
    )
    .await
    .expect("close");

    let closed_ev: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM activity_events WHERE event_code = 'wo.closed' AND source_record_id = ? ORDER BY id DESC LIMIT 1",
            [closed.id.to_string().into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    let archive_id = archive_record(
        &db,
        ArchiveInput {
            source_module: "wo".into(),
            source_record_id: closed.id.to_string(),
            archive_class: "operational_history".into(),
            source_state: Some("closed".into()),
            archive_reason_code: "completed".into(),
            archived_by_id: Some(actor),
            restore_policy: "not_allowed".into(),
            restore_until_at: None,
            payload_json: serde_json::json!({ "wo_id": closed.id }),
            workflow_history_json: None,
            attachment_manifest_json: None,
            config_version_refs_json: None,
            search_text: None,
        },
    )
    .await
    .expect("archive");

    let _ = emit_activity_event(
        &db,
        ActivityEventInput {
            event_class: "operational".into(),
            event_code: "arc.archived".into(),
            source_module: "arc".into(),
            source_record_type: Some("archive_item".into()),
            source_record_id: Some(archive_id.to_string()),
            entity_scope_id: None,
            actor_id: Some(actor),
            severity: "info".into(),
            summary_json: None,
            correlation_id: Some(corr.clone()),
            visibility_scope: "global".into(),
        },
    )
    .await;

    let arc_ev: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM activity_events WHERE event_code = 'arc.archived' ORDER BY id DESC LIMIT 1",
            [],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "id")
        .expect("id");

    insert_event_link(&db, iot_id, di_ev).await;
    insert_event_link(&db, di_ev, wo_ev).await;
    insert_event_link(&db, wo_ev, closed_ev).await;
    insert_event_link(&db, closed_ev, arc_ev).await;

    let chain = build_event_chain(
        &db,
        &EventChainInput {
            root_event_id: iot_id,
            root_table: "activity_events".into(),
        },
    )
    .await
    .expect("chain");

    let codes: Vec<String> = chain
        .events
        .iter()
        .filter_map(|n| n.event_code.clone())
        .collect();
    assert_eq!(codes.len(), 5, "codes={codes:?}");
    let expected = [
        "iot.threshold_exceeded",
        "di.submitted",
        "wo.created",
        "wo.closed",
        "arc.archived",
    ];
    assert_eq!(codes.as_slice(), expected);

    for w in chain.events.windows(2) {
        assert!(w[0].happened_at <= w[1].happened_at, "ordering");
    }

    let acnt: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM audit_events".to_string(),
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert!(acnt >= 1);

    let arc_items: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM archive_items WHERE source_module = 'wo' AND source_record_id = ?",
            [closed.id.to_string().into()],
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert_eq!(arc_items, 1);

    emit_event(
        &db,
        NotificationEventInput {
            source_module: "wo".into(),
            source_record_id: Some(closed.id.to_string()),
            event_code: "wo.assigned".into(),
            category_code: "wo_assigned".into(),
            severity: "info".into(),
            dedupe_key: None,
            payload_json: None,
            title: "Chain".into(),
            body: None,
            action_url: None,
        },
    )
    .await
    .expect("notif");

    let ncnt: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM notifications".to_string(),
        ))
        .await
        .expect("q")
        .expect("r")
        .try_get("", "c")
        .expect("c");
    assert!(ncnt >= 1);
}
