use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::errors::AppError;
use crate::inventory::domain::{
    ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, CreateProcurementRequisitionInput,
    CreatePurchaseOrderFromRequisitionInput, CreateRepairableOrderInput, InventoryArticleFilter, InventoryArticleInput,
    InventoryIssueInput, InventoryReleaseReservationInput, InventoryReserveInput, InventoryReturnInput, InventoryStockAdjustInput,
    InventoryStockFilter, InventoryTransferInput, PostInventoryCountSessionInput, ReceiveGoodsInput, ReceivePurchaseOrderLineInput,
    ReverseInventoryCountSessionInput, RunInventoryReconciliationInput, TransitionInventoryCountSessionInput,
    TransitionProcurementRequisitionInput, TransitionPurchaseOrderInput, TransitionRepairableOrderInput, UpsertInventoryCountLineInput,
};
use crate::inventory::{controls, procurement, queries};

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
    crate::db::seeder::seed_system_data(&db)
        .await
        .expect("seed system data");
    db
}

async fn first_active_lookup_value_id(db: &DatabaseConnection, domain_key: &str) -> i64 {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = ?
               AND lv.is_active = 1
               AND lv.deleted_at IS NULL
             ORDER BY lv.sort_order ASC, lv.id ASC
             LIMIT 1",
            [domain_key.into()],
        ))
        .await
        .expect("query lookup")
        .expect("lookup exists");
    row.try_get("", "id").expect("lookup id")
}

async fn main_location_id(db: &DatabaseConnection) -> i64 {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1".to_string(),
        ))
        .await
        .expect("query main location")
        .expect("main location");
    row.try_get("", "id").expect("main location id")
}

async fn create_test_article(db: &DatabaseConnection, code: &str) -> i64 {
    let unit_value_id = first_active_lookup_value_id(db, "inventory.unit_of_measure").await;
    let stocking_type_value_id = first_active_lookup_value_id(db, "inventory.stocking_type").await;
    let tax_category_value_id = first_active_lookup_value_id(db, "inventory.tax_category").await;
    let created = queries::create_article(
        db,
        InventoryArticleInput {
            article_code: code.to_string(),
            article_name: format!("Article {code}"),
            family_id: None,
            unit_value_id,
            criticality_value_id: None,
            stocking_type_value_id,
            tax_category_value_id,
            procurement_category_value_id: None,
            preferred_warehouse_id: None,
            preferred_location_id: None,
            min_stock: 1.0,
            max_stock: Some(20.0),
            reorder_point: 5.0,
            safety_stock: 0.0,
            is_active: Some(true),
        },
    )
    .await
    .expect("create article");
    created.id
}

async fn first_supplier_id(db: &DatabaseConnection) -> i64 {
    if let Some(row) = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM external_companies WHERE is_active = 1 ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
        .expect("query supplier")
    {
        return row.try_get("", "id").expect("supplier id");
    }

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO external_companies (name, service_domain, is_active)
         VALUES ('Supplier Test', 'inventory', 1)"
            .to_string(),
    ))
    .await
    .expect("insert supplier");
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM external_companies WHERE name = 'Supplier Test' LIMIT 1".to_string(),
        ))
        .await
        .expect("query inserted supplier")
        .expect("inserted supplier row");
    row.try_get("", "id").expect("supplier id")
}

#[tokio::test]
async fn reservation_lifecycle_keeps_balance_and_transaction_invariants() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-RES-LIFE").await;
    let location_id = main_location_id(&db).await;

    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 10.0,
            reason: Some("seed for lifecycle test".to_string()),
        },
    )
    .await
    .expect("adjust stock");

    let reservation = queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id,
            quantity: 6.0,
            source_type: "WO".to_string(),
            source_id: Some(9001),
            source_ref: Some("WO-9001".to_string()),
            notes: Some("plan reserve".to_string()),
        },
    )
    .await
    .expect("reserve");

    queries::issue_reserved_stock(
        &db,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity: 4.0,
            source_type: Some("WO".to_string()),
            source_id: Some(9001),
            source_ref: Some("WO-9001".to_string()),
            notes: Some("usage".to_string()),
        },
    )
    .await
    .expect("issue");

    queries::return_reserved_stock(
        &db,
        InventoryReturnInput {
            reservation_id: reservation.id,
            quantity: 1.0,
            notes: Some("delta correction".to_string()),
        },
    )
    .await
    .expect("return");

    queries::release_stock_reservation(
        &db,
        InventoryReleaseReservationInput {
            reservation_id: reservation.id,
            notes: Some("closeout".to_string()),
        },
    )
    .await
    .expect("release");

    let balances = queries::list_stock_balances(
        &db,
        InventoryStockFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            low_stock_only: None,
        },
    )
    .await
    .expect("list balances");
    let main_balance = balances
        .iter()
        .find(|b| b.location_id == location_id)
        .expect("main balance");
    assert!((main_balance.on_hand_qty - 7.0).abs() < f64::EPSILON);
    assert!((main_balance.reserved_qty - 0.0).abs() < f64::EPSILON);
    assert!((main_balance.available_qty - 7.0).abs() < f64::EPSILON);

    let tx_rows = queries::list_transactions(
        &db,
        crate::inventory::domain::InventoryTransactionFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            source_type: Some("WO".to_string()),
            source_id: Some(9001),
            limit: Some(20),
        },
    )
    .await
    .expect("list transactions");
    assert!(tx_rows.iter().any(|t| t.movement_type == "RESERVE"));
    assert!(tx_rows.iter().any(|t| t.movement_type == "ISSUE"));
    assert!(tx_rows.iter().any(|t| t.movement_type == "RETURN"));
    assert!(tx_rows.iter().any(|t| t.movement_type == "RELEASE"));
}

#[tokio::test]
async fn inactive_article_rejects_stock_mutation_paths() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-INACTIVE").await;
    let location_id = main_location_id(&db).await;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET is_active = 0 WHERE id = ?",
        [article_id.into()],
    ))
    .await
    .expect("deactivate article");

    let adjust_err = queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 1.0,
            reason: Some("should fail".to_string()),
        },
    )
    .await
    .expect_err("adjust on inactive article must fail");
    assert!(matches!(adjust_err, AppError::ValidationFailed(_)));

    let reserve_err = queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id,
            quantity: 1.0,
            source_type: "WO".to_string(),
            source_id: Some(9002),
            source_ref: Some("WO-9002".to_string()),
            notes: None,
        },
    )
    .await
    .expect_err("reserve on inactive article must fail");
    assert!(matches!(reserve_err, AppError::ValidationFailed(_)));
}

#[tokio::test]
async fn transfer_safety_preserves_reserved_and_blocks_double_consumption() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER").await;
    let main_loc = main_location_id(&db).await;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO stock_locations (warehouse_id, code, name, is_default, is_active)
         SELECT warehouse_id, 'AUX-BIN', 'Aux bin', 0, 1
         FROM stock_locations WHERE id = (SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1)"
            .to_string(),
    ))
    .await
    .expect("insert aux location");
    let aux_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM stock_locations WHERE code = 'AUX-BIN' LIMIT 1".to_string(),
        ))
        .await
        .expect("query aux")
        .expect("aux row");
    let aux_loc: i64 = aux_row.try_get("", "id").expect("aux id");

    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id: main_loc,
            delta_qty: 8.0,
            reason: Some("seed transfer".to_string()),
        },
    )
    .await
    .expect("seed stock");

    let reservation = queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id: main_loc,
            quantity: 3.0,
            source_type: "WO".to_string(),
            source_id: Some(9003),
            source_ref: Some("WO-9003".to_string()),
            notes: None,
        },
    )
    .await
    .expect("reserve");

    let transfer_err = queries::transfer_stock(
        &db,
        InventoryTransferInput {
            article_id,
            from_location_id: main_loc,
            to_location_id: aux_loc,
            quantity: 6.0,
            source_type: Some("WO".to_string()),
            source_id: Some(9003),
            source_ref: Some("WO-9003".to_string()),
            notes: Some("should fail due reserved".to_string()),
        },
    )
    .await
    .expect_err("cannot transfer more than available");
    assert!(matches!(transfer_err, AppError::ValidationFailed(_)));

    queries::issue_reserved_stock(
        &db,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity: 3.0,
            source_type: Some("WO".to_string()),
            source_id: Some(9003),
            source_ref: Some("WO-9003".to_string()),
            notes: None,
        },
    )
    .await
    .expect("consume reservation");

    let second_issue_err = queries::issue_reserved_stock(
        &db,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity: 0.1,
            source_type: Some("WO".to_string()),
            source_id: Some(9003),
            source_ref: Some("WO-9003".to_string()),
            notes: None,
        },
    )
    .await
    .expect_err("cannot issue beyond reserved remainder");
    assert!(matches!(second_issue_err, AppError::ValidationFailed(_)));

    let _ = queries::list_articles(
        &db,
        InventoryArticleFilter {
            search: Some("INV-XFER".to_string()),
        },
    )
    .await
    .expect("article still queryable");
}

#[tokio::test]
async fn procurement_requisition_po_gr_flow_keeps_traceability() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-PROC-001").await;
    let location_id = main_location_id(&db).await;
    let supplier_id = first_supplier_id(&db).await;

    let req = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id,
            preferred_location_id: Some(location_id),
            requested_qty: 7.0,
            demand_source_type: "REORDER".to_string(),
            demand_source_id: Some(4444),
            demand_source_ref: Some("ROR-4444".to_string()),
            source_reservation_id: None,
            source_reorder_trigger: Some("threshold_crossed".to_string()),
            reason: Some("auto replenish".to_string()),
            actor_id: None,
        },
    )
    .await
    .expect("create requisition");
    let req = procurement::transition_procurement_requisition(
        &db,
        TransitionProcurementRequisitionInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            next_status: "SUBMITTED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect("submit requisition");
    let req = procurement::transition_procurement_requisition(
        &db,
        TransitionProcurementRequisitionInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            next_status: "APPROVED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect("approve requisition");

    let po = procurement::create_purchase_order_from_requisition(
        &db,
        CreatePurchaseOrderFromRequisitionInput {
            requisition_id: req.id,
            supplier_company_id: Some(supplier_id),
            actor_id: None,
        },
    )
    .await
    .expect("create po");
    let po = procurement::transition_purchase_order(
        &db,
        TransitionPurchaseOrderInput {
            purchase_order_id: po.id,
            expected_row_version: po.row_version,
            next_status: "SUBMITTED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect("submit po");
    let po = procurement::transition_purchase_order(
        &db,
        TransitionPurchaseOrderInput {
            purchase_order_id: po.id,
            expected_row_version: po.row_version,
            next_status: "APPROVED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect("approve po");

    let po_lines = procurement::list_purchase_order_lines(&db, po.id)
        .await
        .expect("list po lines");
    assert_eq!(po_lines.len(), 1);
    assert_eq!(po_lines[0].demand_source_type, "REORDER");
    assert_eq!(po_lines[0].demand_source_ref.as_deref(), Some("ROR-4444"));

    let _gr = procurement::receive_purchase_order_goods(
        &db,
        ReceiveGoodsInput {
            purchase_order_id: po.id,
            lines: vec![ReceivePurchaseOrderLineInput {
                po_line_id: po_lines[0].id,
                article_id,
                location_id,
                received_qty: 7.0,
                accepted_qty: 7.0,
                rejected_qty: 0.0,
                rejection_reason: None,
            }],
            actor_id: None,
        },
    )
    .await
    .expect("receive goods");

    let balances = queries::list_stock_balances(
        &db,
        InventoryStockFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            low_stock_only: None,
        },
    )
    .await
    .expect("balances");
    assert!(
        balances
            .iter()
            .any(|b| b.location_id == location_id && (b.on_hand_qty - 7.0).abs() < f64::EPSILON),
        "GR must post stock increase"
    );
}

#[tokio::test]
async fn repairable_flow_blocks_invalid_close_and_posts_events() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REP-001").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 4.0,
            reason: Some("seed repairable".to_string()),
        },
    )
    .await
    .expect("seed stock");

    let order = procurement::create_repairable_order(
        &db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("send motor for repair".to_string()),
            actor_id: None,
        },
    )
    .await
    .expect("create repairable");

    let invalid_close = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            order_id: order.id,
            expected_row_version: order.row_version,
            next_status: "CLOSED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
            return_location_id: Some(location_id),
        },
    )
    .await
    .expect_err("cannot close from requested");
    assert!(matches!(invalid_close, AppError::ValidationFailed(_)));

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            order_id: order.id,
            expected_row_version: order.row_version,
            next_status: "RELEASED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
            return_location_id: Some(location_id),
        },
    )
    .await
    .expect("release");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            order_id: order.id,
            expected_row_version: order.row_version,
            next_status: "SENT_FOR_REPAIR".to_string(),
            reason: None,
            note: None,
            actor_id: None,
            return_location_id: Some(location_id),
        },
    )
    .await
    .expect("sent");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            order_id: order.id,
            expected_row_version: order.row_version,
            next_status: "RETURNED_FROM_REPAIR".to_string(),
            reason: None,
            note: None,
            actor_id: None,
            return_location_id: Some(location_id),
        },
    )
    .await
    .expect("return");
    let _order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            order_id: order.id,
            expected_row_version: order.row_version,
            next_status: "CLOSED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
            return_location_id: Some(location_id),
        },
    )
    .await
    .expect("close");

    let movements = queries::list_transactions(
        &db,
        crate::inventory::domain::InventoryTransactionFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            source_type: Some("REPAIRABLE_ORDER".to_string()),
            source_id: None,
            limit: Some(50),
        },
    )
    .await
    .expect("movement list");
    assert!(movements.iter().any(|t| t.movement_type == "REPAIRABLE_RELEASE"));
    assert!(movements.iter().any(|t| t.movement_type == "REPAIRABLE_RETURN"));
}

#[tokio::test]
async fn count_session_requires_reviewer_evidence_for_posting() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-CC-001").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 10.0,
            reason: Some("seed count".to_string()),
        },
    )
    .await
    .expect("seed stock");

    let mut session = controls::create_count_session(
        &db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(location_id),
            critical_abs_threshold: Some(2.0),
            actor_id: Some(1),
        },
    )
    .await
    .expect("create session");
    session = controls::transition_count_session(
        &db,
        TransitionInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            next_status: "counting".to_string(),
            reason: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("to counting");
    let line = controls::upsert_count_line(
        &db,
        UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id,
            location_id,
            counted_qty: 6.0,
            variance_reason_code: Some("COUNT_ERROR".to_string()),
        },
    )
    .await
    .expect("upsert line");
    session = controls::transition_count_session(
        &db,
        TransitionInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            next_status: "submitted".to_string(),
            reason: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("submit");
    session = controls::transition_count_session(
        &db,
        TransitionInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            next_status: "approved".to_string(),
            reason: None,
            actor_id: Some(2),
        },
    )
    .await
    .expect("approve");

    let missing_evidence_err = controls::post_count_session(
        &db,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(1),
        },
    )
    .await
    .expect_err("must not post without line evidence");
    assert!(matches!(missing_evidence_err, AppError::ValidationFailed(_)));

    let _approved_line = controls::approve_count_line(
        &db,
        ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: 99,
            reviewer_evidence: "Double-check done with signed sheet".to_string(),
        },
    )
    .await
    .expect("approve line");
    let posted = controls::post_count_session(
        &db,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(1),
        },
    )
    .await
    .expect("post");
    assert_eq!(posted.status, "posted");

    let reversed = controls::reverse_count_session(
        &db,
        ReverseInventoryCountSessionInput {
            session_id: posted.id,
            expected_row_version: posted.row_version,
            reason: "Wrong sheet imported".to_string(),
            actor_id: Some(1),
        },
    )
    .await
    .expect("reverse");
    assert_eq!(reversed.status, "reversed");
}

#[tokio::test]
async fn reconciliation_detects_drift_under_realistic_volume() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;

    for idx in 0..120_i64 {
        let article_id = create_test_article(&db, &format!("INV-BULK-{idx:03}")).await;
        queries::adjust_stock(
            &db,
            InventoryStockAdjustInput {
                article_id,
                location_id,
                delta_qty: (idx + 1) as f64,
                reason: Some("bulk seed".to_string()),
            },
        )
        .await
        .expect("seed bulk");
    }

    let run = controls::run_reconciliation(
        &db,
        RunInventoryReconciliationInput {
            actor_id: Some(1),
            drift_break_threshold: Some(0.1),
        },
    )
    .await
    .expect("reconciliation run");
    assert_eq!(run.checked_rows >= 120, true);

    // Inject a drift directly in balances and verify reconciliation catches it.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE stock_balances
         SET on_hand_qty = on_hand_qty + 5, available_qty = available_qty + 5
         WHERE id = (SELECT id FROM stock_balances ORDER BY id ASC LIMIT 1)"
            .to_string(),
    ))
    .await
    .expect("inject drift");

    let drift_run = controls::run_reconciliation(
        &db,
        RunInventoryReconciliationInput {
            actor_id: Some(1),
            drift_break_threshold: Some(0.1),
        },
    )
    .await
    .expect("drift run");
    assert!(drift_run.drift_rows > 0);
    let findings = controls::list_reconciliation_findings(&db, drift_run.id)
        .await
        .expect("findings");
    assert!(findings.iter().any(|f| f.is_break == 1 && f.drift_qty.abs() >= 5.0));
}

#[tokio::test]
async fn list_stock_balances_includes_synthetic_zero_when_no_balance_row() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-SYN-001").await;
    let location_id = main_location_id(&db).await;

    let balances = queries::list_stock_balances(
        &db,
        InventoryStockFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            low_stock_only: None,
        },
    )
    .await
    .expect("list balances");

    assert_eq!(balances.len(), 1);
    let b = &balances[0];
    assert!(b.id < 0, "synthetic balance uses negative id");
    assert!((b.on_hand_qty - 0.0).abs() < f64::EPSILON);
    assert_eq!(b.location_id, location_id);

    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 3.0,
            reason: Some("seed".to_string()),
        },
    )
    .await
    .expect("adjust");

    let after = queries::list_stock_balances(
        &db,
        InventoryStockFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            low_stock_only: None,
        },
    )
    .await
    .expect("list after adjust");

    assert_eq!(after.len(), 1);
    assert!(after[0].id > 0);
    assert!((after[0].on_hand_qty - 3.0).abs() < f64::EPSILON);
}
