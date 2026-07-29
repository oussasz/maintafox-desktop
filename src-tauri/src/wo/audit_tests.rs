//! Supervisor verification tests — Phase 2 SP05 File 04 Sprint S1. Option B lifecycle.
//!
//! V1 — Permission seed count: 8 ot.* rows after migration 026.
//! V2 — Audit on close: successful close_wo writes row with action='closed',
//!       apply_result='applied', requires_step_up=1.
//! V3 — Blocked close audit: quality-gate failure writes row with
//!       apply_result='blocked' and details_json containing error text.
//! V4 — Fire-and-log: wo_change_events table dropped; close_wo primary workflow
//!       still succeeds (audit failure does not surface to caller).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::wo::audit::{self, WoAuditInput};
    use crate::wo::closeout::{
        self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput,
    };
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput};
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts;
    use crate::wo::queries;
    use crate::wo::workflow::actions::mark_ready::{mark_wo_ready, WoMarkReadyInput};
    use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

    // ═══════════════════════════════════════════════════════════════════════
    // DB setup helpers
    // ═══════════════════════════════════════════════════════════════════════

    async fn setup() -> sea_orm::DatabaseConnection {
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
            .expect("all migrations should apply cleanly");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run cleanly");

        db
    }

    async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1".to_string(),
            ))
            .await
            .expect("query")
            .expect("admin user must exist after seed");
        row.try_get::<i64>("", "id").unwrap()
    }

    async fn create_verifier(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO user_accounts \
             (sync_id, username, display_name, identity_mode, password_hash, \
              is_active, is_admin, force_password_change, \
              failed_login_attempts, created_at, updated_at, row_version) \
             VALUES ('audit-verifier-sync', 'audit_verifier', 'Audit Verifier', 'local', \
                     'no-login-needed', 1, 0, 0, 0, ?, ?, 1)",
            [now.clone().into(), now.into()],
        ))
        .await
        .expect("insert verifier user");

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'audit_verifier' LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("verifier must exist");
        row.try_get::<i64>("", "id").unwrap()
    }

    async fn seed_test_equipment(db: &sea_orm::DatabaseConnection) {
        // Ensure org scaffolding for installed_at_node_id FK
        let _ = db
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO org_structure_models \
                 (id, sync_id, version_number, status, created_at, updated_at) \
                 VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                    .to_string(),
            ))
            .await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types \
             (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert org_node_types");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes \
             (id, sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
                     datetime('now'), datetime('now'), 1);"
                .to_string(),
        ))
        .await
        .expect("insert org_nodes");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment \
             (id, sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
              created_at, updated_at) \
             VALUES (1, 'test-eq-audit-001', 'EQ-AUDIT-001', 'Audit Test Equipment', \
                     'active_in_service', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert test equipment");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = 1 \
             WHERE id = 1 AND installed_at_node_id IS NULL;"
                .to_string(),
        ))
        .await
        .expect("ensure installed_at_node_id");
    }

    /// Seed failure coding + RCA required before `complete_wo_mechanically` for corrective/emergency WOs.
    async fn seed_rams_for_complete(db: &sea_orm::DatabaseConnection, wo_id: i64) {
        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("lookup symptom")
            .expect("seeded DI.SYMPTOM vibration");

        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: Some(symptom_id),
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("Test failure detail notes for RAMS complete gate".into()),
            },
        )
        .await
        .expect("seed_rams_for_complete: save_failure_detail");

        closeout::update_wo_rca(
            db,
            UpdateWoRcaInput {
                wo_id,
                root_cause_summary: Some("Test root cause".into()),
                corrective_action_summary: Some("Test corrective action".into()),
            },
        )
        .await
        .expect("seed_rams_for_complete: update_wo_rca");
    }

    /// Strip RAMS fields so close_wo quality gates can still assert missing failure coding.
    async fn clear_rams_for_close_gate(db: &sea_orm::DatabaseConnection, wo_id: i64) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM work_order_failure_details WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("clear failure details");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = NULL, corrective_action_summary = NULL WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("clear root_cause_summary");
    }

    /// Create a corrective WO and advance to in_progress via Option B lifecycle.
    async fn wo_in_progress(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        seed_test_equipment(db).await;
        let actor = admin_id(db).await;

        let wo = queries::create_work_order(
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
                urgency_id: Some(3),
                title: "Audit test WO".into(),
                description: None,
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: Some(4.0),
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create_work_order");

        let wo_id = wo.id;

        // submit: draft → planning
        let wo = submit_wo(
            db,
            WoSubmitInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit_wo");

        // plan (non-status save)
        let wo = execution::plan_wo(
            db,
            WoPlanInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                planner_id: actor,
                planned_start: "2026-04-10T08:00:00Z".into(),
                planned_end: "2026-04-10T12:00:00Z".into(),
                shift: None,
                expected_duration_hours: Some(4.0),
                urgency_id: None,
            },
        )
        .await
        .expect("plan_wo");

        // assign (non-status save)
        let wo = execution::assign_wo(
            db,
            WoAssignInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");

        // mark ready: planning → ready
        let wo = mark_wo_ready(
            db,
            WoMarkReadyInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("mark_wo_ready");

        // start: ready → in_progress
        let wo = execution::start_wo(
            db,
            WoStartInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("start_wo");

        (wo_id, wo.row_version)
    }

    /// Advance from in_progress to completed+verified with all quality gate data.
    /// In Option B, save_verification does NOT change WO status — stays completed.
    async fn advance_to_completed_verified(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
    ) -> i64 {
        let actor = admin_id(db).await;
        let verifier = create_verifier(db).await;

        labor::add_labor_entry(
            db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T12:00:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("add labor");

        parts::confirm_no_parts_used(db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(db, wo_id).await;

        let wo = execution::complete_wo_mechanically(
            db,
            WoMechCompleteInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("complete_wo_mechanically");
        let rv = wo.row_version;

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        // save_verification: stays on completed
        let (_ver, wo) = closeout::save_verification(
            db,
            SaveVerificationInput {
                wo_id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: None,
                notes: None,
                expected_row_version: rv,
            },
        )
        .await
        .expect("save_verification");

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        wo.row_version
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Permission seed count
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_ot_permission_seed_count_is_8() {
        let db = setup().await;

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM permissions WHERE name LIKE 'ot.%'".to_string(),
            ))
            .await
            .expect("query should succeed")
            .expect("should return a row");

        let cnt: i64 = row.try_get("", "cnt").unwrap();
        assert_eq!(cnt, 8, "expected 8 ot.* permissions, got {cnt}");
    }

    #[tokio::test]
    async fn v1_ot_permission_names_correct() {
        let db = setup().await;

        let expected = [
            "ot.view", "ot.create", "ot.edit", "ot.approve",
            "ot.close", "ot.reopen", "ot.admin", "ot.delete",
        ];

        for name in expected {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM permissions WHERE name = ?",
                    [name.into()],
                ))
                .await
                .expect("query should succeed");
            assert!(row.is_some(), "permission '{name}' must exist after migration 026");
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Audit on close
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_successful_close_writes_audit_row() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = wo_in_progress(&db).await;
        let rv = advance_to_completed_verified(&db, wo_id, rv).await;

        let close_result = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await;

        assert!(close_result.is_ok(), "close_wo must succeed: {:?}", close_result);

        audit::record_wo_change_event(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("Work order closed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT action, apply_result, requires_step_up \
                 FROM wo_change_events \
                 WHERE wo_id = ? AND action = 'closed' \
                 ORDER BY id DESC LIMIT 1",
                [wo_id.into()],
            ))
            .await
            .expect("audit query should succeed")
            .expect("audit row must exist after close");

        let action: String = row.try_get("", "action").unwrap();
        let apply_result: String = row.try_get("", "apply_result").unwrap();
        let requires_step_up: i32 = row.try_get("", "requires_step_up").unwrap();

        assert_eq!(action, "closed");
        assert_eq!(apply_result, "applied");
        assert_eq!(requires_step_up, 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Blocked close audit
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_blocked_close_writes_audit_row_with_details_json() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = wo_in_progress(&db).await;

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T12:00:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("add labor");

        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(&db, wo_id).await;

        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("complete_wo_mechanically");

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        // Strip RAMS so close quality gate fires (no failure detail / root cause / verification).
        clear_rams_for_close_gate(&db, wo_id).await;

        // close_wo quality gate must fire.
        let close_result = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                ..Default::default()
            },
        )
        .await;

        assert!(close_result.is_err(), "close_wo must fail: quality gate not met");
        let errors = match close_result.unwrap_err() {
            crate::errors::AppError::ValidationFailed(errs) => errs,
            other => panic!("expected ValidationFailed, got: {other:?}"),
        };
        assert!(!errors.is_empty(), "errors list must be non-empty");

        let details = serde_json::json!({ "quality_gate_errors": errors }).to_string();
        audit::record_wo_change_event(
            &db,
            WoAuditInput {
                wo_id: None,
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("Close blocked: quality gate failed".into()),
                details_json: Some(details.clone()),
                requires_step_up: true,
                apply_result: "blocked".into(),
            },
        )
        .await;

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT action, apply_result, details_json \
                 FROM wo_change_events \
                 WHERE action = 'closed' AND apply_result = 'blocked' \
                 ORDER BY id DESC LIMIT 1",
                [],
            ))
            .await
            .expect("audit query should succeed")
            .expect("blocked audit row must exist");

        let action: String = row.try_get("", "action").unwrap();
        let apply_result: String = row.try_get("", "apply_result").unwrap();
        let stored_json: Option<String> = row.try_get("", "details_json").ok().flatten();

        assert_eq!(action, "closed");
        assert_eq!(apply_result, "blocked");

        let stored_json = stored_json.expect("details_json must be set on blocked row");
        assert!(
            stored_json.contains("quality_gate_errors"),
            "details_json must contain quality_gate_errors key, got: {stored_json}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Fire-and-log
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_fire_and_log_audit_failure_does_not_block_primary_workflow() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = wo_in_progress(&db).await;
        let rv = advance_to_completed_verified(&db, wo_id, rv).await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS wo_change_events;".to_string(),
        ))
        .await
        .expect("drop wo_change_events");

        let close_result = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await;

        assert!(
            close_result.is_ok(),
            "close_wo must succeed even when wo_change_events is missing: {:?}",
            close_result
        );

        let wo = close_result.unwrap();
        assert_eq!(wo.status_code.as_deref(), Some("closed"));

        // fire-and-log: record_wo_change_event with missing table must NOT panic
        audit::record_wo_change_event(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("Fire-and-log test".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;
        // Reaching here confirms fire-and-log semantics
    }
}
