//! L3-21: only the stock right moves stock. A part used by someone without it is "à sortir": no
//! stock movement, the storekeeper is told, issues it (confirming or correcting the quantity,
//! audited) or cancels it with a reason (audited), and the work order is not closed meanwhile.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::errors::AppError;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::parts::{
        self, to_issue_codes, CancelPartToIssueInput, IssuePartToIssueInput, StockAuthority, WoPart,
    };
    use crate::wo::queries;
    use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

    use StockAuthority::{MayMoveStock, RequestOnly};

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

    async fn exec(db: &sea_orm::DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await
            .expect("exec");
    }

    async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
        scalar_i64(
            db,
            "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1",
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
             VALUES (1, 'test-eq-to-issue', 'EQ-TI-001', 'Parts to issue equipment', 'active_in_service', 1, datetime('now'), datetime('now'))",
        ] {
            exec(db, sql, vec![]).await;
        }
    }

    /// A work order in planning. Returns its id.
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
                title: "Parts to issue WO".into(),
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
        submit_wo(
            db,
            WoSubmitInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit to planning");
        wo.id
    }

    async fn set_wo_status(db: &sea_orm::DatabaseConnection, wo_id: i64, code: &str) {
        exec(
            db,
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = ?) WHERE id = ?",
            vec![code.into(), wo_id.into()],
        )
        .await;
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
        exec(
            db,
            "INSERT INTO articles (entity_sync_id, article_code, article_name, unit_value_id, \
             stocking_type_value_id, tax_category_value_id, min_stock, reorder_point, safety_stock, \
             is_active, row_version) VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 1, 1)",
            vec![
                format!("art-{code}").into(),
                code.into(),
                format!("Article {code}").into(),
                unit.into(),
                stocking.into(),
                tax.into(),
            ],
        )
        .await;
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
                reason_code: Some("seed for parts to issue test".to_string()),
                notes: None,
                source_ref: None,
            },
        )
        .await
        .expect("seed stock");
        (article_id, location_id)
    }

    async fn on_hand(db: &sea_orm::DatabaseConnection, article_id: i64, location_id: i64) -> f64 {
        scalar_f64(
            db,
            "SELECT on_hand_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![article_id.into(), location_id.into()],
        )
        .await
    }

    async fn issues(db: &sea_orm::DatabaseConnection, article_id: i64) -> i64 {
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM inventory_transactions WHERE article_id = ? AND movement_type = 'ISSUE'",
            vec![article_id.into()],
        )
        .await
    }

    async fn audit_count(db: &sea_orm::DatabaseConnection, wo_id: i64, action: &str) -> i64 {
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM wo_change_events WHERE wo_id = ? AND action = ?",
            vec![wo_id.into(), action.into()],
        )
        .await
    }

    async fn notices(db: &sea_orm::DatabaseConnection, wo_id: i64) -> i64 {
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM notification_events WHERE category_code = ? AND source_record_id = ?",
            vec![parts::PART_TO_ISSUE_CATEGORY.into(), wo_id.to_string().into()],
        )
        .await
    }

    fn refusal_code(err: AppError) -> String {
        match err {
            AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
            AppError::ValidationFailed(messages) => messages
                .first()
                .map(|m| m.split([':', ' ']).next().unwrap_or_default().to_string())
                .unwrap_or_default(),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn input(wo_id: i64, article_id: i64, location_id: Option<i64>, qty: f64) -> parts::AddPartInput {
        parts::AddPartInput {
            wo_id,
            article_id: Some(article_id),
            article_ref: None,
            quantity_planned: qty,
            unit_cost: None,
            stock_location_id: location_id,
            auto_reserve: Some(true),
            notes: None,
            origin: None,
            is_required: true,
        }
    }

    /// A WO in progress with a stock line added during execution by someone without the stock
    /// right: (wo_id, article_id, location_id, line).
    async fn execution_line_to_issue(
        db: &sea_orm::DatabaseConnection,
        code: &str,
        location: bool,
    ) -> (i64, i64, i64, WoPart) {
        let wo_id = wo_in_planning(db).await;
        let (article_id, location_id) = seed_stock_article(db, code, 10.0).await;
        set_wo_status(db, wo_id, "in_progress").await;
        let line = parts::add_planned_part(
            db,
            input(wo_id, article_id, location.then_some(location_id), 3.0),
            RequestOnly,
        )
        .await
        .expect("add the used part");
        (wo_id, article_id, location_id, line)
    }

    async fn close_refusals(db: &sea_orm::DatabaseConnection, wo_id: i64) -> Vec<String> {
        let evidence = crate::wo::closeout::CloseEvidence {
            closer_user_id: Some(admin_id(db).await),
            no_downtime_attestation: None,
            no_downtime_attestation_reason: None,
            fmeca_override_reason: None,
            fmeca_override_signer_id: None,
            fmeca_override_signer_password: crate::wo::closeout::SignerPassword::Given(None),
        };
        let rpn = crate::reliability::advanced_rams::queries::fmeca_rpn_critical_threshold_i64_in(db)
            .await
            .expect("rpn");
        crate::wo::closeout::close_preflight(db, wo_id, &evidence, rpn)
            .await
            .expect("preflight")
            .0
    }

    #[tokio::test]
    async fn a_part_added_without_the_stock_right_moves_no_stock_and_is_to_issue() {
        let db = crate::db::test_db::seeded().await;
        let (wo_id, article_id, location_id, line) = execution_line_to_issue(&db, "TI-ADD-1", true).await;

        assert_eq!(issues(&db, article_id).await, 0, "no stock movement");
        assert_eq!(on_hand(&db, article_id, location_id).await, 10.0);
        assert_eq!(line.pending_issue_qty, 3.0);
        assert_eq!(
            line.quantity_used,
            Some(3.0),
            "the technician's quantity stays on the line"
        );
        assert_eq!(line.quantity_issued, 0.0);
        assert_eq!(line.reservation_id, None);
        assert_eq!(line.consumption_status, "used");
        assert_eq!(notices(&db, wo_id).await, 1, "the stock right holders are told");

        let listed = parts::list_parts_to_issue(&db).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].part.id, line.id);
        assert_eq!(listed[0].wo_title.as_deref(), Some("Parts to issue WO"));
        assert!(listed[0].location_label.is_some());
    }

    #[tokio::test]
    async fn a_part_added_with_the_stock_right_still_leaves_the_store_at_once() {
        let db = crate::db::test_db::seeded().await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "TI-ADD-2", 10.0).await;
        set_wo_status(&db, wo_id, "in_progress").await;
        let line = parts::add_planned_part(&db, input(wo_id, article_id, Some(location_id), 3.0), MayMoveStock)
            .await
            .expect("add");
        assert_eq!(issues(&db, article_id).await, 1);
        assert_eq!(on_hand(&db, article_id, location_id).await, 7.0);
        assert_eq!(line.pending_issue_qty, 0.0);
        assert_eq!(line.quantity_issued, 3.0);
        assert_eq!(notices(&db, wo_id).await, 0);
    }

    /// `auto_reserve: false` does not bypass the rule: the line is still "à sortir".
    #[tokio::test]
    async fn auto_reserve_off_does_not_hide_a_used_stock_part() {
        let db = crate::db::test_db::seeded().await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "TI-ADD-3", 10.0).await;
        set_wo_status(&db, wo_id, "in_progress").await;
        let mut add = input(wo_id, article_id, Some(location_id), 2.0);
        add.auto_reserve = Some(false);
        let line = parts::add_planned_part(&db, add, RequestOnly).await.expect("add");
        assert_eq!(line.pending_issue_qty, 2.0);
        assert_eq!(issues(&db, article_id).await, 0);
    }

    #[tokio::test]
    async fn usage_recorded_without_the_stock_right_issues_nothing_from_the_reservation() {
        let db = crate::db::test_db::seeded().await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "TI-USE-1", 10.0).await;
        // A planned line only reserves, whoever adds it.
        let planned = parts::add_planned_part(&db, input(wo_id, article_id, Some(location_id), 2.0), RequestOnly)
            .await
            .expect("plan");
        assert!(planned.reservation_id.is_some());
        set_wo_status(&db, wo_id, "in_progress").await;

        let used = parts::record_actual_usage(&db, planned.id, 2.0, None, RequestOnly)
            .await
            .expect("record use");
        assert_eq!(issues(&db, article_id).await, 0, "nothing leaves the store");
        assert_eq!(on_hand(&db, article_id, location_id).await, 10.0);
        assert_eq!(used.pending_issue_qty, 2.0);
        assert_eq!(used.quantity_issued, 0.0);
        assert_eq!(notices(&db, wo_id).await, 1);

        // The technician corrects the quantity down: still nothing issued, less to issue, no new notice.
        let corrected = parts::record_actual_usage(&db, planned.id, 1.0, None, RequestOnly)
            .await
            .expect("correct use");
        assert_eq!(corrected.pending_issue_qty, 1.0);
        assert_eq!(notices(&db, wo_id).await, 1);

        // The storekeeper issues it from the reservation.
        let actor = admin_id(&db).await;
        let issued = parts::issue_part_to_issue(
            &db,
            IssuePartToIssueInput {
                wo_part_id: planned.id,
                quantity: 1.0,
                stock_location_id: None,
                correction_reason: None,
            },
            actor,
        )
        .await
        .expect("issue");
        assert_eq!(issues(&db, article_id).await, 1);
        assert_eq!(on_hand(&db, article_id, location_id).await, 9.0);
        assert_eq!(issued.pending_issue_qty, 0.0);
        assert_eq!(issued.quantity_issued, 1.0);
        assert_eq!(issued.quantity_used, Some(1.0));
        assert_eq!(audit_count(&db, wo_id, "part_to_issue_issued").await, 1);
        assert!(parts::list_parts_to_issue(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn without_the_stock_right_a_used_quantity_below_what_was_issued_is_refused() {
        let db = crate::db::test_db::seeded().await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "TI-USE-2", 10.0).await;
        let planned = parts::add_planned_part(&db, input(wo_id, article_id, Some(location_id), 2.0), MayMoveStock)
            .await
            .expect("plan");
        set_wo_status(&db, wo_id, "in_progress").await;
        parts::record_actual_usage(&db, planned.id, 2.0, None, MayMoveStock)
            .await
            .expect("storekeeper issues 2");
        let err = parts::record_actual_usage(&db, planned.id, 1.0, None, RequestOnly)
            .await
            .expect_err("a return is the storekeeper's");
        assert_eq!(refusal_code(err), "GATE_WO_PART_RETURN_REQUIRED");
        assert_eq!(on_hand(&db, article_id, location_id).await, 8.0, "no return recorded");
    }

    #[tokio::test]
    async fn a_corrected_issue_needs_a_reason_and_is_audited_with_both_quantities() {
        let db = crate::db::test_db::seeded().await;
        let (wo_id, article_id, location_id, line) = execution_line_to_issue(&db, "TI-FIX-1", true).await;
        let actor = admin_id(&db).await;
        let issue = |quantity: f64, reason: Option<&str>| IssuePartToIssueInput {
            wo_part_id: line.id,
            quantity,
            stock_location_id: None,
            correction_reason: reason.map(str::to_string),
        };

        let err = parts::issue_part_to_issue(&db, issue(2.0, None), actor)
            .await
            .expect_err("a correction without a reason");
        assert_eq!(refusal_code(err), to_issue_codes::REASON_REQUIRED);
        let err = parts::issue_part_to_issue(&db, issue(0.0, Some("nothing taken")), actor)
            .await
            .expect_err("zero is a cancellation");
        assert_eq!(refusal_code(err), to_issue_codes::QUANTITY_INVALID);
        assert_eq!(issues(&db, article_id).await, 0);

        let issued = parts::issue_part_to_issue(&db, issue(2.0, Some("only two taken from the bin")), actor)
            .await
            .expect("corrected issue");
        assert_eq!(issued.quantity_used, Some(2.0), "the storekeeper's figure is the use");
        assert_eq!(issued.quantity_issued, 2.0);
        assert_eq!(issued.pending_issue_qty, 0.0);
        assert_eq!(on_hand(&db, article_id, location_id).await, 8.0);
        let details: String = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT details_json FROM wo_change_events WHERE wo_id = ? AND action = 'part_to_issue_issued'",
                [wo_id.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap();
        let details: serde_json::Value = serde_json::from_str(&details).unwrap();
        assert_eq!(details["quantity_requested"], 3.0);
        assert_eq!(details["quantity_issued"], 2.0);
        assert_eq!(details["corrected"], true);
        assert_eq!(details["correction_reason"], "only two taken from the bin");

        let err = parts::issue_part_to_issue(&db, issue(1.0, None), actor)
            .await
            .expect_err("nothing left to issue");
        assert_eq!(refusal_code(err), to_issue_codes::NOTHING_TO_ISSUE);
    }

    #[tokio::test]
    async fn a_line_without_a_location_is_issued_from_the_location_the_storekeeper_chooses() {
        let db = crate::db::test_db::seeded().await;
        let (_, article_id, location_id, line) = execution_line_to_issue(&db, "TI-LOC-1", false).await;
        assert_eq!(line.pending_issue_qty, 3.0);
        let actor = admin_id(&db).await;
        let err = parts::issue_part_to_issue(
            &db,
            IssuePartToIssueInput {
                wo_part_id: line.id,
                quantity: 3.0,
                stock_location_id: None,
                correction_reason: None,
            },
            actor,
        )
        .await
        .expect_err("no location");
        assert_eq!(refusal_code(err), to_issue_codes::LOCATION_REQUIRED);
        let issued = parts::issue_part_to_issue(
            &db,
            IssuePartToIssueInput {
                wo_part_id: line.id,
                quantity: 3.0,
                stock_location_id: Some(location_id),
                correction_reason: None,
            },
            actor,
        )
        .await
        .expect("issue");
        assert_eq!(issued.stock_location_id, Some(location_id));
        assert_eq!(on_hand(&db, article_id, location_id).await, 7.0);
    }

    #[tokio::test]
    async fn a_cancelled_quantity_to_issue_needs_a_reason_and_withdraws_the_use() {
        let db = crate::db::test_db::seeded().await;
        let (wo_id, article_id, _, line) = execution_line_to_issue(&db, "TI-CAN-1", true).await;
        let actor = admin_id(&db).await;
        let err = parts::cancel_part_to_issue(
            &db,
            CancelPartToIssueInput {
                wo_part_id: line.id,
                reason: " no ".into(),
            },
            actor,
        )
        .await
        .expect_err("reason too short");
        assert_eq!(refusal_code(err), to_issue_codes::REASON_REQUIRED);

        let cancelled = parts::cancel_part_to_issue(
            &db,
            CancelPartToIssueInput {
                wo_part_id: line.id,
                reason: "part taken from the previous job's leftovers".into(),
            },
            actor,
        )
        .await
        .expect("cancel");
        assert_eq!(cancelled.consumption_status, parts::PART_STATUS_CANCELLED);
        assert_eq!(cancelled.quantity_used, Some(0.0));
        assert_eq!(cancelled.pending_issue_qty, 0.0);
        assert_eq!(issues(&db, article_id).await, 0);
        assert_eq!(audit_count(&db, wo_id, "part_to_issue_cancelled").await, 1);

        // A planned line goes back to planned: still to be used or marked not used.
        let (planned_article, location_id) = seed_stock_article(&db, "TI-CAN-2", 5.0).await;
        set_wo_status(&db, wo_id, "planning").await;
        let planned = parts::add_planned_part(&db, input(wo_id, planned_article, Some(location_id), 1.0), RequestOnly)
            .await
            .expect("plan");
        set_wo_status(&db, wo_id, "in_progress").await;
        parts::record_actual_usage(&db, planned.id, 1.0, None, RequestOnly)
            .await
            .expect("use");
        let back = parts::cancel_part_to_issue(
            &db,
            CancelPartToIssueInput {
                wo_part_id: planned.id,
                reason: "not needed after all".into(),
            },
            actor,
        )
        .await
        .expect("cancel planned use");
        assert_eq!(back.consumption_status, "pending");
        assert_eq!(back.quantity_used, None);
        assert!(
            back.reservation_id.is_some(),
            "the reservation stays for the planned line"
        );
    }

    #[tokio::test]
    async fn the_work_order_is_not_closed_while_a_part_is_to_issue() {
        let db = crate::db::test_db::seeded().await;
        let (wo_id, _, _, line) = execution_line_to_issue(&db, "TI-CLOSE-1", true).await;
        set_wo_status(&db, wo_id, "completed").await;
        let gate = |refusals: Vec<String>| {
            refusals
                .iter()
                .any(|r| r.starts_with("GATE_CLOSEOUT_PARTS_TO_ISSUE:count=1"))
        };
        assert!(
            gate(close_refusals(&db, wo_id).await),
            "blocked while the part is to issue"
        );

        parts::issue_part_to_issue(
            &db,
            IssuePartToIssueInput {
                wo_part_id: line.id,
                quantity: 3.0,
                stock_location_id: None,
                correction_reason: None,
            },
            admin_id(&db).await,
        )
        .await
        .expect("issue");
        assert!(!gate(close_refusals(&db, wo_id).await), "issued: no longer blocking");
    }

    #[tokio::test]
    async fn issued_stock_is_never_dropped_by_marking_the_line_not_used() {
        let db = crate::db::test_db::seeded().await;
        let wo_id = wo_in_planning(&db).await;
        let (article_id, location_id) = seed_stock_article(&db, "TI-NU-1", 10.0).await;
        let planned = parts::add_planned_part(&db, input(wo_id, article_id, Some(location_id), 2.0), MayMoveStock)
            .await
            .expect("plan");
        set_wo_status(&db, wo_id, "in_progress").await;
        parts::record_actual_usage(&db, planned.id, 2.0, None, MayMoveStock)
            .await
            .expect("issue 2");
        let reason_id = scalar_i64(
            &db,
            "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE UPPER(TRIM(rd.code)) = 'WORK.PART_UNUSED_REASON' AND rv.is_active = 1 \
               AND rs.status = 'published' AND LOWER(rv.code) <> 'other' LIMIT 1",
            vec![],
        )
        .await;
        let err = parts::mark_part_not_used(
            &db,
            parts::MarkPartNotUsedInput {
                wo_part_id: planned.id,
                not_used_reason_id: reason_id,
                not_used_comment: None,
                actor_id: None,
            },
        )
        .await
        .expect_err("stock left the store");
        assert_eq!(refusal_code(err), "GATE_WO_PART_RETURN_REQUIRED");
    }

    /// Sync: the storekeeper's cancellation of a line that is entirely "à sortir" on this PC is no
    /// conflict (nothing left the store); a line with issued stock still is.
    #[tokio::test]
    async fn a_received_cancellation_of_a_line_only_to_issue_is_not_a_conflict() {
        let db = crate::db::test_db::seeded().await;
        let (_, _, _, line) = execution_line_to_issue(&db, "TI-SYNC-1", true).await;
        exec(
            &db,
            "UPDATE work_order_parts SET entity_sync_id = 'ti-sync-line' WHERE id = ?",
            vec![line.id.into()],
        )
        .await;
        let mut payload = serde_json::Map::new();
        payload.insert("consumption_status".into(), serde_json::json!("cancelled"));
        payload.insert("quantity_used".into(), serde_json::json!(0.0));
        payload.insert("quantity_issued".into(), serde_json::json!(0.0));
        let verdict = parts::inbound_part_line_check(&db, "ti-sync-line", &payload)
            .await
            .expect("check");
        assert!(verdict.is_none(), "nothing left the store: no conflict");

        exec(
            &db,
            "UPDATE work_order_parts SET quantity_issued = 1, pending_issue_qty = 2 WHERE id = ?",
            vec![line.id.into()],
        )
        .await;
        let verdict = parts::inbound_part_line_check(&db, "ti-sync-line", &payload)
            .await
            .expect("check");
        assert!(verdict.is_some(), "issued stock: a conflict for a person");
    }
}
