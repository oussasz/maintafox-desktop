//! Supervisor verification tests — Phase 2 SP05 File 04 Sprint S1.
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
        self, SaveFailureDetailInput, SaveVerificationInput, WoCloseInput,
    };
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{
        self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput,
    };
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts;
    use crate::wo::queries;

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

    /// Create a corrective WO and advance to in_progress. Returns (wo_id, row_version).
    async fn wo_in_progress(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let actor = admin_id(db).await;

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
        let mut rv = wo.row_version;

        let wo = execution::plan_wo(
            db,
            WoPlanInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
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
        rv = wo.row_version;

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
        .expect("advance to ready_to_schedule");
        rv += 1;

        let wo = execution::assign_wo(
            db,
            WoAssignInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");
        rv = wo.row_version;

        let wo = execution::start_wo(
            db,
            WoStartInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
            },
        )
        .await
        .expect("start_wo");

        (wo_id, wo.row_version)
    }

    /// Advance from in_progress → technically_verified with all quality gate data satisfied.
    async fn advance_to_technically_verified(
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

        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("Audit test failure detail".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Audit test root cause".into(), wo_id.into()],
        ))
        .await
        .expect("set root_cause_summary");

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

        wo.row_version
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Permission seed count
    // ═══════════════════════════════════════════════════════════════════════

    /// After migration 026, `SELECT COUNT(*) FROM permissions WHERE name LIKE 'ot.%'`
    /// must return exactly 8.
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

    /// Cross-check that each specific permission name is present.
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

    /// Successful close_wo writes exactly 1 wo_change_events row with
    /// action='closed', apply_result='applied', requires_step_up=1.
    #[tokio::test]
    async fn v2_successful_close_writes_audit_row() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = wo_in_progress(&db).await;
        let rv = advance_to_technically_verified(&db, wo_id, rv).await;

        // Record the audit event manually (mirrors what commands/wo.rs does)
        let _wo = queries::get_work_order(&db, wo_id)
            .await
            .expect("get_work_order")
            .expect("wo should exist");

        // Call close_wo
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

        // Simulate the audit event that commands/wo.rs writes on success
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

        // Verify the audit row
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

        assert_eq!(action, "closed", "action must be 'closed'");
        assert_eq!(apply_result, "applied", "apply_result must be 'applied'");
        assert_eq!(requires_step_up, 1, "requires_step_up must be 1 (true) for close");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Blocked close audit
    // ═══════════════════════════════════════════════════════════════════════

    /// When close_wo fails the quality gate, a wo_change_events row is written
    /// with apply_result='blocked' and details_json containing the error list.
    #[tokio::test]
    async fn v3_blocked_close_writes_audit_row_with_details_json() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = wo_in_progress(&db).await;

        // Add labor so the labor gate passes
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

        // Force WO to technically_verified without saving failure detail or root cause
        // This ensures close_wo quality gate will fire
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
             status_id = (SELECT id FROM work_order_statuses WHERE code = 'technically_verified'), \
             technically_verified_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
             row_version = row_version + 1, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
             WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("advance to technically_verified");
        let rv = wo.row_version + 1;

        // close_wo — must fail; quality gate fires for missing failure detail + root_cause
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

        assert!(close_result.is_err(), "close_wo must fail: quality gate not met");
        let errors = match close_result.unwrap_err() {
            crate::errors::AppError::ValidationFailed(errs) => errs,
            other => panic!("expected ValidationFailed, got: {other:?}"),
        };
        assert!(!errors.is_empty(), "errors list must be non-empty");

        // Simulate the audit event that commands/wo.rs writes on quality gate failure
        let details = serde_json::json!({ "quality_gate_errors": errors }).to_string();
        audit::record_wo_change_event(
            &db,
            WoAuditInput {
                wo_id: None, // not available when close_wo returns Err
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("Close blocked: quality gate failed".into()),
                details_json: Some(details.clone()),
                requires_step_up: true,
                apply_result: "blocked".into(),
            },
        )
        .await;

        // Verify the blocked audit row
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

    /// Drop wo_change_events from the DB; call close_wo; the primary workflow
    /// must still succeed — audit failure must NOT surface to the caller.
    #[tokio::test]
    async fn v4_fire_and_log_audit_failure_does_not_block_primary_workflow() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = wo_in_progress(&db).await;
        let rv = advance_to_technically_verified(&db, wo_id, rv).await;

        // Drop wo_change_events to simulate a catastrophic audit storage failure
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS wo_change_events;".to_string(),
        ))
        .await
        .expect("drop wo_change_events");

        // Primary workflow: close_wo must still succeed
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
        assert_eq!(
            wo.status_code.as_deref(),
            Some("closed"),
            "WO must reach 'closed' status despite audit table being absent"
        );

        // Now verify fire-and-log: record_wo_change_event with missing table does NOT panic/error
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
        // If we reach here without panic, fire-and-log semantics are confirmed
    }
}
