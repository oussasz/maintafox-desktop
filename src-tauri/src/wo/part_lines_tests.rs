//! Planned part lines: cancel (never delete) and required / optional switch while the work order
//! is prepared — reservation released once, audit written in the same transaction, a new line
//! when the part is added again, and the coded refusals.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::errors::AppError;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::parts::{self, part_line_codes, CancelPlannedPartInput, SetPlannedPartRequiredInput};
    use crate::wo::queries;
    use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

    async fn scalar_i64(db: &sea_orm::DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
        db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await
            .expect("query")
            .expect("row")
            .try_get_by_index::<i64>(0)
            .expect("i64")
    }

    async fn scalar_f64(db: &sea_orm::DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> f64 {
        db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await
            .expect("query")
            .expect("row")
            .try_get_by_index::<f64>(0)
            .expect("f64")
    }

    async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
        scalar_i64(
            db,
            "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1",
            vec![],
        )
        .await
    }

    /// An active login with no role at all (so without the WO edit right).
    async fn user_without_rights(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts \
             (entity_sync_id, username, display_name, identity_mode, password_hash, \
              is_active, is_admin, force_password_change, failed_login_attempts, \
              created_at, updated_at, row_version) \
             VALUES ('part-line-viewer-sync', 'part_line_viewer', 'Viewer', 'local', 'x', 1, 0, 0, 0, ?, ?, 1)",
            [now.clone().into(), now.into()],
        ))
        .await
        .expect("insert user");
        scalar_i64(
            db,
            "SELECT id FROM user_accounts WHERE username = 'part_line_viewer'",
            vec![],
        )
        .await
    }

    async fn seed_equipment(db: &sea_orm::DatabaseConnection) {
        for sql in [
            "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
            "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', datetime('now'), datetime('now'), 1)",
            "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at) \
             VALUES (1, 'test-eq-part-lines', 'EQ-PL-001', 'Part lines equipment', 'active_in_service', 1, datetime('now'), datetime('now'))",
        ] {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                .await
                .expect("seed equipment scaffolding");
        }
    }

    /// A work order in the planning status. Returns its id.
    async fn wo_in_planning(db: &sea_orm::DatabaseConnection) -> i64 {
        seed_equipment(db).await;
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
                title: "Part lines WO".into(),
                description: None,
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: Some(2.0),
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");
        let wo = submit_wo(
            db,
            WoSubmitInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit to planning");
        assert_eq!(wo.status_code.as_deref(), Some("planning"));
        wo.id
    }

    async fn set_wo_status(db: &sea_orm::DatabaseConnection, wo_id: i64, code: &str) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = ?) WHERE id = ?",
            [code.into(), wo_id.into()],
        ))
        .await
        .expect("set status");
    }

    async fn lookup_value_id(db: &sea_orm::DatabaseConnection, domain_key: &str) -> i64 {
        scalar_i64(
            db,
            "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL \
             ORDER BY lv.sort_order, lv.id LIMIT 1",
            vec![domain_key.into()],
        )
        .await
    }

    /// Article with `qty` on hand at the seeded MAIN-BIN location. Returns (article_id, location_id).
    async fn seed_stock_article(db: &sea_orm::DatabaseConnection, code: &str, qty: f64) -> (i64, i64) {
        let unit = crate::reference::units::unit_id_by_code(db, "U").await.expect("unit U");
        let stocking = lookup_value_id(db, "inventory.stocking_type").await;
        let tax = lookup_value_id(db, "inventory.tax_category").await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id, \
             stocking_type_value_id, tax_category_value_id, min_stock, reorder_point, safety_stock, \
             is_active, row_version) VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 1, 1)",
            [
                format!("art-{code}").into(),
                code.into(),
                format!("Article {code}").into(),
                unit.into(),
                stocking.into(),
                tax.into(),
            ],
        ))
        .await
        .expect("insert article");
        let article_id = scalar_i64(db, "SELECT id FROM articles WHERE article_code = ?", vec![code.into()]).await;
        let location_id = scalar_i64(
            db,
            "SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1",
            vec![],
        )
        .await;
        crate::inventory::queries::adjust_stock(
            db,
            crate::inventory::domain::InventoryStockAdjustInput {
                article_id,
                location_id,
                delta_qty: qty,
                reason_code: Some("seed for part lines test".to_string()),
                notes: None,
                source_ref: None,
            },
        )
        .await
        .expect("seed stock");
        (article_id, location_id)
    }

    async fn reserved_qty(db: &sea_orm::DatabaseConnection, article_id: i64, location_id: i64) -> f64 {
        scalar_f64(
            db,
            "SELECT reserved_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![article_id.into(), location_id.into()],
        )
        .await
    }

    async fn add_planned(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        article_id: i64,
        location_id: i64,
        qty: f64,
    ) -> parts::WoPart {
        parts::add_planned_part(
            db,
            parts::AddPartInput {
                wo_id,
                article_id: Some(article_id),
                article_ref: None,
                quantity_planned: qty,
                unit_cost: Some(5.0),
                stock_location_id: Some(location_id),
                auto_reserve: Some(true),
                notes: None,
                origin: None,
                is_required: true,
            },
        )
        .await
        .expect("add planned part")
    }

    fn refusal_code(err: AppError) -> String {
        match err {
            AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
            other => panic!("expected a coded refusal, got {other:?}"),
        }
    }

    async fn audit_count(db: &sea_orm::DatabaseConnection, wo_id: i64, action: &str) -> i64 {
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM wo_change_events WHERE wo_id = ? AND action = ?",
            vec![wo_id.into(), action.into()],
        )
        .await
    }

    #[tokio::test]
    async fn cancel_releases_the_reservation_exactly_once_and_writes_the_audit() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-BRG-01", 10.0).await;
        let part = add_planned(&db, wo_id, article_id, location_id, 3.0).await;
        let reservation_id = part.reservation_id.expect("planned line reserves stock");
        assert_eq!(reserved_qty(&db, article_id, location_id).await, 3.0);
        assert!(part.unit_symbol.is_some(), "the stock unit comes with the line");

        let cancelled = parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
            .await
            .expect("cancel");
        assert_eq!(cancelled.consumption_status, parts::PART_STATUS_CANCELLED);
        assert_eq!(cancelled.reservation_id, None);
        assert_eq!(cancelled.quantity_reserved, 0.0);
        assert_eq!(
            cancelled.quantity_planned, 3.0,
            "the planned quantity stays for the history"
        );
        assert_eq!(reserved_qty(&db, article_id, location_id).await, 0.0);
        let status: String = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT status FROM stock_reservations WHERE id = ?",
                [reservation_id.into()],
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "status")
            .expect("status");
        assert_eq!(status, "released");

        // Audit: who, line, quantity — one entry; one history event.
        let audit = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT actor_id, acted_at, details_json FROM wo_change_events \
                  WHERE wo_id = ? AND action = 'part_line_cancelled'",
                [wo_id.into()],
            ))
            .await
            .expect("query")
            .expect("audit row");
        assert_eq!(audit.try_get::<Option<i64>>("", "actor_id").unwrap(), Some(actor));
        assert!(!audit.try_get::<String>("", "acted_at").unwrap().is_empty());
        let details: serde_json::Value =
            serde_json::from_str(&audit.try_get::<String>("", "details_json").unwrap()).unwrap();
        assert_eq!(details["wo_part_id"], part.id);
        assert_eq!(details["quantity_planned"], 3.0);
        assert_eq!(details["released_reservation_id"], reservation_id);
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM work_order_execution_events WHERE work_order_id = ? AND event_type = 'part_cancelled'",
                vec![wo_id.into()],
            )
            .await,
            1
        );

        // A second cancel is refused and releases nothing more.
        let err = parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
            .await
            .expect_err("already cancelled");
        assert_eq!(refusal_code(err), part_line_codes::CANCELLED);
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM inventory_transactions WHERE reservation_id = ? AND movement_type = 'RELEASE'",
                vec![reservation_id.into()],
            )
            .await,
            1,
            "the reservation is released exactly once"
        );
        assert_eq!(reserved_qty(&db, article_id, location_id).await, 0.0);
        assert_eq!(audit_count(&db, wo_id, "part_line_cancelled").await, 1);
    }

    #[tokio::test]
    async fn adding_the_part_again_makes_one_new_line_with_one_reservation() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-SEAL-02", 10.0).await;
        let first = add_planned(&db, wo_id, article_id, location_id, 4.0).await;
        parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: first.id }, actor)
            .await
            .expect("cancel");

        let again = add_planned(&db, wo_id, article_id, location_id, 2.0).await;
        assert_ne!(again.id, first.id, "a new line, the cancelled one is kept");
        assert_eq!(reserved_qty(&db, article_id, location_id).await, 2.0);
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM stock_reservations WHERE source_id = ? AND article_id = ? AND status <> 'released'",
                vec![wo_id.into(), article_id.into()],
            )
            .await,
            1,
            "one live reservation, no duplicate"
        );

        let lines = parts::list_wo_parts(&db, wo_id).await.expect("list");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].consumption_status, "cancelled");
        assert_eq!(lines[1].consumption_status, "pending");

        // The cancelled line leaves the planned counts and cost.
        let adherence = crate::wo::plan_adherence::get_plan_adherence(&db, wo_id)
            .await
            .expect("adherence");
        assert_eq!(adherence.parts_planned, 1);
        assert!((adherence.planned_parts_cost - 10.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn only_cancelled_planned_lines_leave_no_planned_part_to_settle() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-FLT-03", 5.0).await;
        let part = add_planned(&db, wo_id, article_id, location_id, 1.0).await;
        parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
            .await
            .expect("cancel");
        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("a cancelled line is not a planned part waiting for Used / Not used");
    }

    #[tokio::test]
    async fn a_line_with_recorded_use_cannot_be_cancelled_or_switched() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-USED-04", 5.0).await;
        let part = add_planned(&db, wo_id, article_id, location_id, 2.0).await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_parts SET quantity_used = 1, consumption_status = 'used' WHERE id = ?",
            [part.id.into()],
        ))
        .await
        .expect("record use");

        let err = parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
            .await
            .expect_err("used line");
        assert_eq!(refusal_code(err), part_line_codes::HAS_USE);
        let err = parts::set_planned_part_required(
            &db,
            SetPlannedPartRequiredInput {
                wo_part_id: part.id,
                is_required: false,
            },
            actor,
        )
        .await
        .expect_err("used line");
        assert_eq!(refusal_code(err), part_line_codes::HAS_USE);
        assert_eq!(
            reserved_qty(&db, article_id, location_id).await,
            2.0,
            "nothing released"
        );
        assert_eq!(audit_count(&db, wo_id, "part_line_cancelled").await, 0);
    }

    #[tokio::test]
    async fn a_work_order_past_planning_refuses_both_changes() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-STAT-05", 5.0).await;
        let part = add_planned(&db, wo_id, article_id, location_id, 2.0).await;
        for status in ["ready", "in_progress"] {
            set_wo_status(&db, wo_id, status).await;
            let err = parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
                .await
                .expect_err("not in planning");
            assert_eq!(refusal_code(err), part_line_codes::NOT_IN_PLANNING);
            let err = parts::set_planned_part_required(
                &db,
                SetPlannedPartRequiredInput {
                    wo_part_id: part.id,
                    is_required: false,
                },
                actor,
            )
            .await
            .expect_err("not in planning");
            assert_eq!(refusal_code(err), part_line_codes::NOT_IN_PLANNING);
        }
        assert_eq!(reserved_qty(&db, article_id, location_id).await, 2.0);
    }

    #[tokio::test]
    async fn a_person_without_the_wo_edit_right_is_refused() {
        let db = crate::db::test_db::seeded().await;
        let viewer = user_without_rights(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-RIGHT-06", 5.0).await;
        let part = add_planned(&db, wo_id, article_id, location_id, 2.0).await;

        let err = parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, viewer)
            .await
            .expect_err("no right");
        assert_eq!(refusal_code(err), part_line_codes::RIGHT_REQUIRED);
        let err = parts::set_planned_part_required(
            &db,
            SetPlannedPartRequiredInput {
                wo_part_id: part.id,
                is_required: false,
            },
            viewer,
        )
        .await
        .expect_err("no right");
        assert_eq!(refusal_code(err), part_line_codes::RIGHT_REQUIRED);
        let line = &parts::list_wo_parts(&db, wo_id).await.expect("list")[0];
        assert_eq!(line.consumption_status, "pending");
        assert!(line.is_required);
        assert_eq!(reserved_qty(&db, article_id, location_id).await, 2.0);
    }

    #[tokio::test]
    async fn an_execution_added_line_is_not_cancelled_here() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let part = parts::add_planned_part(
            &db,
            parts::AddPartInput {
                wo_id,
                article_id: None,
                article_ref: Some("Free-text gasket".into()),
                quantity_planned: 1.0,
                unit_cost: None,
                stock_location_id: None,
                auto_reserve: Some(false),
                notes: None,
                origin: Some("execution_added".into()),
                is_required: true,
            },
        )
        .await
        .expect("add");
        let err = parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
            .await
            .expect_err("execution-added line");
        assert_eq!(refusal_code(err), part_line_codes::NOT_PLANNED_LINE);
    }

    #[tokio::test]
    async fn switching_required_optional_is_audited_and_a_cancelled_line_takes_no_use() {
        let db = crate::db::test_db::seeded().await;
        let actor = admin_id(&db).await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "PL-NEC-07", 5.0).await;
        let part = add_planned(&db, wo_id, article_id, location_id, 2.0).await;

        let switched = parts::set_planned_part_required(
            &db,
            SetPlannedPartRequiredInput {
                wo_part_id: part.id,
                is_required: false,
            },
            actor,
        )
        .await
        .expect("switch to optional");
        assert!(!switched.is_required);
        assert_eq!(audit_count(&db, wo_id, "part_line_necessity_changed").await, 1);

        // The same value again changes and records nothing.
        parts::set_planned_part_required(
            &db,
            SetPlannedPartRequiredInput {
                wo_part_id: part.id,
                is_required: false,
            },
            actor,
        )
        .await
        .expect("no-op");
        assert_eq!(audit_count(&db, wo_id, "part_line_necessity_changed").await, 1);

        parts::cancel_planned_part(&db, CancelPlannedPartInput { wo_part_id: part.id }, actor)
            .await
            .expect("cancel");
        set_wo_status(&db, wo_id, "in_progress").await;
        let err = parts::record_actual_usage(&db, part.id, 1.0, None)
            .await
            .expect_err("cancelled line");
        assert_eq!(refusal_code(err), part_line_codes::CANCELLED);
    }

    #[test]
    fn a_line_received_as_cancelled_with_recorded_use_is_a_conflict() {
        let payload = |status: &str, used: f64, issued: f64| {
            serde_json::json!({
                "consumption_status": status,
                "quantity_used": used,
                "quantity_issued": issued,
            })
            .as_object()
            .cloned()
            .unwrap()
        };
        assert!(parts::inbound_part_line_conflict(&payload("cancelled", 0.0, 0.0)).is_none());
        assert!(parts::inbound_part_line_conflict(&payload("cancelled", 1.0, 0.0)).is_some());
        assert!(parts::inbound_part_line_conflict(&payload("cancelled", 0.0, 2.0)).is_some());
        assert!(parts::inbound_part_line_conflict(&payload("used", 1.0, 1.0)).is_none());
    }
}
