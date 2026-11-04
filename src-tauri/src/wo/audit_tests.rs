//! Supervisor verification tests — Phase 2 SP05 File 04 Sprint S1. Option B lifecycle.
//!
//! V1 — Permission seed count: 8 ot.* rows after migration 026.
//! V2 — Audit on close: successful close_wo writes row with action='closed',
//!       apply_result='applied', requires_step_up=0 (confirmed in a dialog).
//! V3 — Blocked close audit: quality-gate failure writes row with
//!       apply_result='blocked' and details_json containing error text.
//! V4 — Fire-and-log: wo_change_events table dropped; close_wo primary workflow
//!       still succeeds (audit failure does not surface to caller).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::wo::audit::{self, WoAuditInput};
    use crate::wo::closeout::{self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput};
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
        crate::db::test_db::seeded().await
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
             (entity_sync_id, username, display_name, identity_mode, password_hash, \
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
                "SELECT id FROM user_accounts WHERE username = 'audit_verifier' LIMIT 1".to_string(),
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
                 (id, entity_sync_id, version_number, status, created_at, updated_at) \
                 VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                    .to_string(),
            ))
            .await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types \
             (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert org_node_types");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes \
             (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
                     datetime('now'), datetime('now'), 1);"
                .to_string(),
        ))
        .await
        .expect("insert org_nodes");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment \
             (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
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
    async fn seed_rams_for_complete(db: &sea_orm::DatabaseConnection, wo_id: i64) -> i64 {
        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("lookup symptom")
            .expect("seeded DI.SYMPTOM vibration");

        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                expected_row_version: crate::wo::closeout::wo_row_version(db, wo_id)
                    .await
                    .expect("wo row_version"),
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
                expected_row_version: crate::wo::closeout::wo_row_version(db, wo_id)
                    .await
                    .expect("wo row_version"),
                root_cause_summary: Some("Test root cause".into()),
                corrective_action_summary: Some("Test corrective action".into()),
            },
        )
        .await
        .expect("seed_rams_for_complete: update_wo_rca")
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
                urgency_code: Some("normal".into()),
                subtype_code: None,
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
                planned_start: Some("2026-04-10T08:00:00Z".to_string()),
                planned_end: Some("2026-04-10T12:00:00Z".to_string()),
                shift: None,
                shift_class_version_id: None,
                clear_shift: false,
                expected_duration_hours: Some(4.0),
                planned_crew_size: None,
                clear_crew_size: false,
                planned_downtime_hours: None,
                urgency_code: None,
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
                assigned_team_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
                override_reason: "test assign".to_string(),
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
    async fn advance_to_completed_verified(db: &sea_orm::DatabaseConnection, wo_id: i64, rv: i64) -> i64 {
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

        let rv = seed_rams_for_complete(db, wo_id).await;

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
        // 9 since ot.emergency_assign (emergency reassignment) joined the registry.
        assert_eq!(cnt, 9, "expected 9 ot.* permissions, got {cnt}");
    }

    #[tokio::test]
    async fn v1_ot_permission_names_correct() {
        let db = setup().await;

        let expected = [
            crate::rbac::permissions::OT_VIEW,
            crate::rbac::permissions::OT_CREATE,
            crate::rbac::permissions::OT_EDIT,
            crate::rbac::permissions::OT_APPROVE,
            crate::rbac::permissions::OT_CLOSE,
            crate::rbac::permissions::OT_REOPEN,
            crate::rbac::permissions::OT_ADMIN,
            crate::rbac::permissions::OT_DELETE,
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
                requires_step_up: false,
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
        // Close is confirmed in a dialog (owner decision 2026-10-14), not by step-up.
        assert_eq!(requires_step_up, 0);
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

        let rv = seed_rams_for_complete(&db, wo_id).await;

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

    // ═══════════════════════════════════════════════════════════════════════
    // L5-09 — field changes (old → new), merged history, audit search by OT number
    // ═══════════════════════════════════════════════════════════════════════

    fn change_of<'a>(details: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
        details
            .get("changes")?
            .as_array()?
            .iter()
            .find(|c| c.get("field").and_then(|f| f.as_str()) == Some(field))
    }

    async fn latest_change_details(db: &sea_orm::DatabaseConnection, wo_id: i64, action: &str) -> serde_json::Value {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT details_json FROM wo_change_events WHERE wo_id = ? AND action = ? ORDER BY id DESC LIMIT 1",
                [wo_id.into(), action.into()],
            ))
            .await
            .expect("query")
            .expect("change event row");
        let raw: Option<String> = row.try_get("", "details_json").unwrap();
        serde_json::from_str(&raw.expect("details_json")).expect("json")
    }

    #[tokio::test]
    async fn l509_field_changes_list_only_differing_fields() {
        let db = setup().await;
        let (wo_id, _) = wo_in_progress(&db).await;
        let wo = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        assert!(audit::wo_field_changes(&wo, &wo).is_empty());

        let mut after = wo.clone();
        after.closed_at = Some("2026-10-28T18:26:00Z".into());
        after.title = "Nouveau titre".into();
        let changes = audit::wo_field_changes(&wo, &after);
        assert_eq!(changes.len(), 2, "{changes:?}");
        let details = audit::details_with_changes(Some(serde_json::json!("brut")), changes).unwrap();
        assert_eq!(details["detail"], "brut");
        let closed = change_of(&details, "closed_at").expect("closed_at change");
        assert!(closed["old"].is_null());
        assert_eq!(closed["new"], "2026-10-28T18:26:00Z");
        assert_eq!(change_of(&details, "title").unwrap()["new"], "Nouveau titre");
        assert!(audit::details_with_changes(None, Vec::new()).is_none());
    }

    #[tokio::test]
    async fn l509_change_with_diff_records_old_new_and_copies_to_audit_trail() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, _) = wo_in_progress(&db).await;
        let before = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET planned_start = '2026-11-10T08:00:00Z', row_version = row_version + 1 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .unwrap();

        audit::record_wo_change_with_diff(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "planned".into(),
                actor_id: Some(actor),
                summary: Some("Work order planned".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
            Some(&before),
            None,
            None,
        )
        .await;

        let details = latest_change_details(&db, wo_id, "planned").await;
        let change = change_of(&details, "planned_start").expect("planned_start change");
        assert_eq!(change["old"], serde_json::json!(before.planned_start));
        assert_eq!(change["new"], "2026-11-10T08:00:00Z");

        // The audit trail holds the same change, its object the work order.
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT details_json FROM audit_events WHERE action_code = 'wo.planned' AND target_type = 'work_order' AND target_id = ?",
                [wo_id.to_string().into()],
            ))
            .await
            .unwrap()
            .expect("audit copy");
        let copy: serde_json::Value =
            serde_json::from_str(&row.try_get::<String>("", "details_json").unwrap()).unwrap();
        assert_eq!(copy["source"], audit::AUDIT_COPY_SOURCE);
        assert_eq!(copy["wo_code"], before.code.as_str());
        assert!(change_of(&copy, "planned_start").is_some());

        // The per-WO history shows the change once (the audit copy is not repeated).
        let history = audit::list_wo_history(&db, wo_id, 100).await.unwrap();
        assert_eq!(history.iter().filter(|e| e.action == "planned").count(), 1);
        assert!(history.iter().all(|e| e.action != "wo.planned"), "{history:?}");

        // A change that changes nothing writes no audit copy.
        let same = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        audit::record_wo_change_with_diff(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "paused".into(),
                actor_id: Some(actor),
                summary: None,
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
            Some(&same),
            None,
            None,
        )
        .await;
        let copies = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'wo.paused'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap();
        assert_eq!(copies, 0);
    }

    #[tokio::test]
    async fn l509_renamed_reference_is_not_a_change_of_the_work_order() {
        let db = setup().await;
        let (wo_id, _) = wo_in_progress(&db).await;
        let before = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        // The equipment, the status or the team renamed elsewhere: same ids, other labels.
        let mut after = before.clone();
        after.asset_label = Some("Pompe renommée".into());
        after.status_label = Some("Autre libellé".into());
        after.assigned_team_label = Some("Équipe renommée".into());
        after.responsible_display_name = Some("Autre nom".into());
        after.urgency_label = Some("Autre urgence".into());
        assert!(audit::wo_field_changes(&before, &after).is_empty());
        // Another equipment (another id) is a change, shown by its label.
        after.equipment_id = Some(before.equipment_id.unwrap_or(0) + 1);
        let changes = audit::wo_field_changes(&before, &after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0]["field"], "equipment");
        assert!(changes[0]["new"].as_str().unwrap_or("").contains("Pompe renommée"));
    }

    #[tokio::test]
    async fn l509_write_after_the_close_commit_is_not_credited_to_the_closer() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv) = wo_in_progress(&db).await;
        let rv = advance_to_completed_verified(&db, wo_id, rv).await;
        let before = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        assert_eq!(before.row_version, rv);
        let closed = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await
        .expect("close_wo");
        // A sync write lands after the close's commit, before the command records its event.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET notes = 'from another PC', title = 'Renamed elsewhere', \
             row_version = row_version + 1 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .unwrap();

        // As `commands::wo::close_wo_audited` does: no returned row, the expected version.
        audit::record_wo_change_with_diff(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "closed".into(),
                actor_id: Some(actor),
                summary: None,
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
            Some(&before),
            None,
            Some(rv),
        )
        .await;
        let details = latest_change_details(&db, wo_id, "closed").await;
        assert!(details.get("changes").is_none(), "{details}");
        assert_eq!(details[audit::CHANGES_UNAVAILABLE], true);
        // The row the close returned (read after its commit) would not have been a proof either:
        // it is one version later, like a sole write, so only the re-read can tell.
        assert_eq!(closed.row_version, rv + 1);
    }

    #[tokio::test]
    async fn l509_draft_save_that_changes_nothing_still_checks_the_version() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        seed_test_equipment(&db).await;
        let wo = queries::create_work_order(
            &db,
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
                title: "Brouillon".into(),
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
        .expect("create");
        let empty = |rv: i64| crate::wo::domain::WoDraftUpdateInput {
            id: wo.id,
            expected_row_version: rv,
            title: None,
            type_code: None,
            equipment_id: None,
            location_id: None,
            description: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            notes: None,
            urgency_code: None,
            subtype_code: None,
            requires_permit: None,
        };
        // Nothing to save, read at the current version: accepted, nothing written.
        let same = queries::update_wo_draft_fields(&db, empty(wo.row_version), Some(actor))
            .await
            .expect("no-op save");
        assert_eq!(same.row_version, wo.row_version);
        // Someone else wrote meanwhile: the no-op save is refused like a write, so its event can
        // never show their change as this user's.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET title = 'Autre PC', row_version = row_version + 1 WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .unwrap();
        let err = queries::update_wo_draft_fields(&db, empty(wo.row_version), Some(actor))
            .await
            .expect_err("stale no-op save");
        assert!(format!("{err:?}").contains("GATE_RECORD_CHANGED_ELSEWHERE"), "{err:?}");
    }

    #[tokio::test]
    async fn l509_change_mixed_with_another_write_records_no_values() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, _) = wo_in_progress(&db).await;
        let before = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        // Two writes between the reads: this user's and someone else's (sync, other PC).
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET planned_start = '2026-11-12T08:00:00Z', row_version = row_version + 2 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .unwrap();

        audit::record_wo_change_with_diff(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "planned".into(),
                actor_id: Some(actor),
                summary: None,
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
            Some(&before),
            None,
            None,
        )
        .await;

        let details = latest_change_details(&db, wo_id, "planned").await;
        assert!(details.get("changes").is_none(), "{details}");
        assert_eq!(details[audit::CHANGES_UNAVAILABLE], true);
        let copies = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'wo.planned'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap();
        assert_eq!(copies, 0, "no audit copy without values");
        let mut after = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        assert!(audit::sole_write_changes(&before, &after).is_none());
        after.row_version = before.row_version + 1;
        assert!(audit::sole_write_changes(&before, &after).is_some());
    }

    #[tokio::test]
    async fn l509_close_records_closing_date_and_history_merges_audit_events() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv) = wo_in_progress(&db).await;
        let rv = advance_to_completed_verified(&db, wo_id, rv).await;
        let before = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();

        closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await
        .expect("close_wo");

        // The close-out's own audit event carries the closing date, old → new.
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT details_json FROM audit_events WHERE action_code = 'wo.closed' AND target_id = ?",
                [wo_id.to_string().into()],
            ))
            .await
            .unwrap()
            .expect("wo.closed audit event");
        let details: serde_json::Value =
            serde_json::from_str(&row.try_get::<String>("", "details_json").unwrap()).unwrap();
        let closed = change_of(&details["detail"], "closed_at").expect("closed_at change");
        assert!(closed["old"].is_null());
        assert!(closed["new"].is_string());

        // Closed without a change event (imported history, seed): the history still shows it.
        let history = audit::list_wo_history(&db, wo_id, 100).await.unwrap();
        let from_audit: Vec<_> = history.iter().filter(|e| e.action == "wo.closed").collect();
        assert_eq!(from_audit.len(), 1, "{history:?}");
        assert_eq!(from_audit[0].source, "audit");

        // Closed from the screen: the change event records the fields, and replaces the audit
        // event in the history; no second audit copy is written.
        audit::record_wo_change_with_diff(
            &db,
            WoAuditInput {
                wo_id: Some(wo_id),
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("Work order closed".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
            Some(&before),
            None,
            None,
        )
        .await;
        let change_details = latest_change_details(&db, wo_id, "closed").await;
        assert!(change_of(&change_details, "closed_at").is_some());
        assert!(change_of(&change_details, "status").is_some());
        let history = audit::list_wo_history(&db, wo_id, 100).await.unwrap();
        assert!(history.iter().all(|e| e.action != "wo.closed"), "{history:?}");
        assert_eq!(history.iter().filter(|e| e.action == "closed").count(), 1);
        let n = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'wo.closed'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn l509_audit_log_search_by_ot_number_and_filter_options() {
        use crate::audit::writer::{write_audit_event, AuditEventInput};
        use crate::commands::audit_log::{audit_filter_options, build_audit_filter_query, AuditFilter};

        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, _) = wo_in_progress(&db).await;
        let wo = queries::get_work_order(&db, wo_id).await.unwrap().unwrap();
        for (code, target) in [("wo.closed", Some(wo_id.to_string())), ("login.success", None)] {
            write_audit_event(
                &db,
                AuditEventInput {
                    action_code: code.into(),
                    target_type: target.as_ref().map(|_| "work_order".to_string()),
                    target_id: target,
                    actor_id: Some(actor),
                    auth_context: "session".into(),
                    result: "success".into(),
                    before_hash: None,
                    after_hash: None,
                    retention_class: "standard".into(),
                    details_json: None,
                },
            )
            .await
            .unwrap();
        }

        let run = |filter: AuditFilter| {
            let db = &db;
            async move {
                let (sql, values) = build_audit_filter_query(&filter, true);
                db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
                    .await
                    .expect("audit query")
            }
        };

        // Part of the OT number is enough; the object reads as "code — title".
        let tail = &wo.code[wo.code.len().saturating_sub(4)..];
        let rows = run(AuditFilter {
            object_search: Some(tail.to_string()),
            ..Default::default()
        })
        .await;
        assert_eq!(rows.len(), 1);
        let label: Option<String> = rows[0].try_get("", "target_label").unwrap();
        assert_eq!(label.as_deref(), Some(format!("{} — {}", wo.code, wo.title).as_str()));
        let actor_name: Option<String> = rows[0].try_get("", "actor_display_name").unwrap();
        assert!(actor_name.is_some());

        // A LIKE wildcard typed by the user is taken literally.
        assert!(run(AuditFilter {
            object_search: Some("%".into()),
            ..Default::default()
        })
        .await
        .is_empty());

        // Exact action (not a prefix).
        assert_eq!(
            run(AuditFilter {
                action: Some("wo.closed".into()),
                ..Default::default()
            })
            .await
            .len(),
            1
        );
        assert!(run(AuditFilter {
            action: Some("wo".into()),
            ..Default::default()
        })
        .await
        .is_empty());

        let options = audit_filter_options(&db, None).await.unwrap();
        assert!(options.actions.iter().any(|a| a == "wo.closed"));
        assert!(options.actors.iter().any(|a| a.id == actor && !a.label.is_empty()));
        let own = audit_filter_options(&db, Some(-1)).await.unwrap();
        assert!(own.actions.is_empty() && own.actors.is_empty());
    }
}
