use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::inventory::domain::{
    ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, CreateProcurementRequisitionInput,
    CreatePurchaseOrderFromRequisitionInput, CreateRepairableOrderInput, InventoryArticleFilter, InventoryArticleInput,
    InventoryIssueInput, InventoryReleaseReservationInput, InventoryReserveInput, InventoryReturnInput,
    InventoryStockAdjustInput, InventoryStockFilter, InventoryTransferInput, PostInventoryCountSessionInput,
    ReceiveGoodsInput, ReceivePurchaseOrderLineInput, ReverseInventoryCountSessionInput,
    RunInventoryReconciliationInput, TransitionInventoryCountSessionInput, TransitionProcurementRequisitionInput,
    TransitionPurchaseOrderInput, TransitionRepairableOrderInput, UpsertInventoryCountLineInput,
};
use crate::inventory::{controls, procurement, queries};

async fn setup_db() -> DatabaseConnection {
    crate::db::test_db::seeded().await
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
    let unit_value_id = crate::reference::units::unit_id_by_code(db, "U").await.expect("unit U");
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
            manufacturer_name: None,
            manufacturer_part_number: None,
            oem_part_number: None,
            replenishment_policy_code: None,
            economic_order_qty: None,
            minimum_order_qty: None,
            maximum_order_qty: None,
            order_multiple_qty: None,
            lead_time_days: None,
            review_period_days: None,
            abc_class_code: None,
            xyz_class_code: None,
            is_critical_spare: None,
            requires_expiration: None,
            shelf_life_days: None,
            requires_batch_tracking: None,
            is_active: Some(true),
            barcode: None,
            requires_serial_tracking: None,
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
            reason_code: Some("seed for lifecycle test".to_string()),
            notes: None,
            source_ref: None,
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
            reason_code: Some("should fail".to_string()),
            notes: None,
            source_ref: None,
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

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_locations (entity_sync_id, warehouse_id, code, name, is_default, is_active)
         SELECT ?, warehouse_id, 'AUX-BIN', 'Aux bin', 0, 1
         FROM stock_locations WHERE id = (SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1)",
        [crate::sync::identity::new_entity_sync_id().into()],
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
            reason_code: Some("seed transfer".to_string()),
            notes: None,
            source_ref: None,
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
            lot_id: None,
        },
        None,
    )
    .await
    .expect_err("cannot transfer more than available");
    assert!(matches!(transfer_err, AppError::OrgValidationFailed(_)));

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
            demand_source_line_id: None,
            source_reservation_id: None,
            source_reorder_trigger: Some("threshold_crossed".to_string()),
            purchase_priority: None,
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
            actor_id: Some(1),
        },
    )
    .await
    .expect("approve requisition");

    let po = procurement::create_purchase_order_from_requisition(
        &db,
        CreatePurchaseOrderFromRequisitionInput {
            requisition_id: req.id,
            supplier_id: None,
            supplier_company_id: Some(supplier_id),
            actor_id: None,
            currency_code: None,
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
            actor_id: Some(1),
        },
    )
    .await
    .expect("approve po");
    assert!(
        po.expected_delivery_date.is_some(),
        "approval must set a delivery promise"
    );

    let po_lines = procurement::list_purchase_order_lines(&db, po.id)
        .await
        .expect("list po lines");
    assert_eq!(po_lines.len(), 1);
    assert_eq!(po_lines[0].demand_source_type, "REORDER");
    assert_eq!(po_lines[0].demand_source_ref.as_deref(), Some("ROR-4444"));
    assert_eq!(po_lines[0].remaining_qty, po_lines[0].ordered_qty);

    let projection = queries::project_stock_impact(&db, article_id, None, -3.0, true)
        .await
        .expect("stock impact projection");
    assert_eq!(projection.incoming_open_po_qty, po_lines[0].ordered_qty);
    assert_eq!(
        projection.projected_on_hand,
        projection.current_on_hand - 3.0 + projection.incoming_open_po_qty
    );

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
                lots: None,
                entered_unit_value_id: None,
            }],
            fulfillment_action: None,
            actor_id: None,
            exchange_rate: None,
            posting_date: None,
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

    let detail = procurement::get_purchase_order_detail(&db, po.id)
        .await
        .expect("po detail");
    assert_eq!(detail.lines.len(), 1);
    assert_eq!(detail.goods_receipts.len(), 1);
    assert!(!detail.state_events.is_empty());
    assert_eq!(detail.grand_total.is_none(), detail.lines[0].unit_price.is_none());
    assert_eq!(detail.grand_total_partial, detail.lines[0].unit_price.is_none());
}

/// Base transition payload; callers override `next_status` and the repair facts they capture.
fn repairable_transition(
    order_id: i64,
    expected_row_version: i64,
    return_location_id: i64,
) -> TransitionRepairableOrderInput {
    TransitionRepairableOrderInput {
        order_id,
        expected_row_version,
        next_status: String::new(),
        reason: None,
        note: None,
        actor_id: None,
        return_location_id: Some(return_location_id),
        serial_number: None,
        vendor_supplier_id: None,
        repair_cost: None,
        warranty_active: None,
        warranty_until: None,
    }
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
            reason_code: Some("seed repairable".to_string()),
            notes: None,
            source_ref: None,
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
            serial_number: Some("SN-REP-001".to_string()),
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create repairable");
    assert_eq!(order.serial_number.as_deref(), Some("SN-REP-001"));

    let invalid_close = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "CLOSED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect_err("cannot close from requested");
    assert!(matches!(invalid_close, AppError::ValidationFailed(_)));

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RELEASED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("release");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SENT_FOR_REPAIR".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("sent");
    assert!(order.sent_at.is_some(), "SENT_FOR_REPAIR must stamp sent_at");

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RETURNED_FROM_REPAIR".to_string(),
            repair_cost: Some(120.0),
            warranty_active: Some(true),
            warranty_until: Some("2027-01-01T00:00:00Z".to_string()),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("return");
    let returned_at = order.returned_at.clone();
    assert!(returned_at.is_some(), "RETURNED_FROM_REPAIR must stamp returned_at");
    assert_eq!(order.repair_cost, Some(120.0));
    assert_eq!(order.warranty_active, 1);

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "CLOSED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("close");
    assert_eq!(
        order.returned_at, returned_at,
        "CLOSED must not overwrite the actual return date"
    );

    let detail = procurement::get_repairable_order_detail(&db, order.id)
        .await
        .expect("repairable detail");
    assert_eq!(detail.history_stats.repair_count, 1);
    assert_eq!(detail.history_stats.avg_cost, Some(120.0));
    assert!(!detail.state_events.is_empty());
    assert!(detail.repair_vs_replace.threshold_ratio > 0.0);

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

async fn on_hand_at(db: &DatabaseConnection, article_id: i64, location_id: i64) -> f64 {
    let balances = queries::list_stock_balances(
        db,
        InventoryStockFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            low_stock_only: None,
        },
    )
    .await
    .expect("list balances");
    balances
        .iter()
        .find(|b| b.location_id == location_id)
        .map(|b| b.on_hand_qty)
        .unwrap_or(0.0)
}

async fn repairable_movements(
    db: &DatabaseConnection,
    article_id: i64,
) -> Vec<crate::inventory::domain::InventoryTransaction> {
    queries::list_transactions(
        db,
        crate::inventory::domain::InventoryTransactionFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            source_type: Some("REPAIRABLE_ORDER".to_string()),
            source_id: None,
            limit: Some(50),
        },
    )
    .await
    .expect("movement list")
}

#[tokio::test]
async fn repairable_cancel_from_requested_is_balance_neutral() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REP-CXL-REQ").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 5.0,
            reason_code: Some("seed".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed");
    let before = on_hand_at(&db, article_id, location_id).await;

    let order = procurement::create_repairable_order(
        &db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("cancel before release".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create");

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "CANCELLED".to_string(),
            reason: Some("not needed".to_string()),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("cancel from requested");
    assert_eq!(order.status, "CANCELLED");
    assert!((on_hand_at(&db, article_id, location_id).await - before).abs() < f64::EPSILON);
    let movements = repairable_movements(&db, article_id).await;
    assert!(!movements.iter().any(|t| t.movement_type == "REPAIRABLE_CANCEL_RESTORE"));
    assert!(!movements.iter().any(|t| t.movement_type == "REPAIRABLE_RELEASE"));
}

#[tokio::test]
async fn repairable_cancel_from_released_restores_source_stock() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REP-CXL-REL").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 5.0,
            reason_code: Some("seed".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed");
    let before = on_hand_at(&db, article_id, location_id).await;

    let order = procurement::create_repairable_order(
        &db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("will cancel after release".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create");

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RELEASED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("release");
    assert!((on_hand_at(&db, article_id, location_id).await - (before - 2.0)).abs() < f64::EPSILON);

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "CANCELLED".to_string(),
            reason: Some("abort after release".to_string()),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("cancel from released");
    assert_eq!(order.status, "CANCELLED");
    assert!((on_hand_at(&db, article_id, location_id).await - before).abs() < f64::EPSILON);
    let movements = repairable_movements(&db, article_id).await;
    assert!(movements.iter().any(|t| t.movement_type == "REPAIRABLE_RELEASE"));
    assert!(movements.iter().any(|t| t.movement_type == "REPAIRABLE_CANCEL_RESTORE"));
}

#[tokio::test]
async fn repairable_scrap_from_sent_is_balance_neutral() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REP-SCRAP-SENT").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 5.0,
            reason_code: Some("seed".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed");
    let before = on_hand_at(&db, article_id, location_id).await;

    let order = procurement::create_repairable_order(
        &db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("scrap at vendor".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RELEASED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("release");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SENT_FOR_REPAIR".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("sent");
    let after_sent = on_hand_at(&db, article_id, location_id).await;
    assert!((after_sent - (before - 2.0)).abs() < f64::EPSILON);

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SCRAPPED".to_string(),
            reason: Some("beyond repair".to_string()),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("scrap from sent");
    assert_eq!(order.status, "SCRAPPED");
    assert!((on_hand_at(&db, article_id, location_id).await - after_sent).abs() < f64::EPSILON);
    let movements = repairable_movements(&db, article_id).await;
    assert!(!movements.iter().any(|t| t.movement_type == "REPAIRABLE_SCRAP_WRITEOFF"));
}

#[tokio::test]
async fn repairable_scrap_from_returned_writes_off_return_stock() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REP-SCRAP-RET").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 5.0,
            reason_code: Some("seed".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed");
    let before = on_hand_at(&db, article_id, location_id).await;

    let order = procurement::create_repairable_order(
        &db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("scrap after return".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RELEASED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("release");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SENT_FOR_REPAIR".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("sent");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RETURNED_FROM_REPAIR".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("return");
    assert!((on_hand_at(&db, article_id, location_id).await - before).abs() < f64::EPSILON);

    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SCRAPPED".to_string(),
            reason: Some("failed inspection".to_string()),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("scrap from returned");
    assert_eq!(order.status, "SCRAPPED");
    assert!((on_hand_at(&db, article_id, location_id).await - (before - 2.0)).abs() < f64::EPSILON);
    let movements = repairable_movements(&db, article_id).await;
    assert!(movements.iter().any(|t| t.movement_type == "REPAIRABLE_SCRAP_WRITEOFF"));
}

#[tokio::test]
async fn repairable_scrap_from_returned_rejects_when_stock_short() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REP-SCRAP-SHORT").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 2.0,
            reason_code: Some("seed".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed");

    let order = procurement::create_repairable_order(
        &db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("scrap short".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RELEASED".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("release");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SENT_FOR_REPAIR".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("sent");
    let order = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "RETURNED_FROM_REPAIR".to_string(),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect("return");

    // Drain stock so scrap cannot write off.
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: -2.0,
            reason_code: Some("drain".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("drain");

    let err = procurement::transition_repairable_order(
        &db,
        TransitionRepairableOrderInput {
            next_status: "SCRAPPED".to_string(),
            reason: Some("should fail".to_string()),
            ..repairable_transition(order.id, order.row_version, location_id)
        },
    )
    .await
    .expect_err("scrap must reject when on-hand is short");
    assert!(matches!(err, AppError::ValidationFailed(_)));
    assert_eq!(
        procurement::get_repairable_order(&db, order.id)
            .await
            .expect("reload")
            .status,
        "RETURNED_FROM_REPAIR",
        "failed scrap must leave status unchanged"
    );
}

#[tokio::test]
async fn requisition_reject_requires_reason_and_is_terminal() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-REQ-REJ").await;
    let location_id = main_location_id(&db).await;
    let req = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id,
            preferred_location_id: Some(location_id),
            requested_qty: 1.0,
            demand_source_type: "REORDER".to_string(),
            demand_source_id: None,
            demand_source_ref: None,
            demand_source_line_id: None,
            source_reservation_id: None,
            source_reorder_trigger: None,
            purchase_priority: Some("NORMAL".to_string()),
            reason: Some("need stock".to_string()),
            actor_id: None,
        },
    )
    .await
    .expect("create req");
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
    .expect("submit");

    let missing = procurement::transition_procurement_requisition(
        &db,
        TransitionProcurementRequisitionInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            next_status: "REJECTED".to_string(),
            reason: Some("   ".to_string()),
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect_err("blank reason must fail");
    assert!(matches!(missing, AppError::ValidationFailed(_)));

    let rejected = procurement::transition_procurement_requisition(
        &db,
        TransitionProcurementRequisitionInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            next_status: "REJECTED".to_string(),
            reason: Some("duplicate request".to_string()),
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect("reject");
    assert_eq!(rejected.status, "REJECTED");

    let reopen = procurement::transition_procurement_requisition(
        &db,
        TransitionProcurementRequisitionInput {
            requisition_id: rejected.id,
            expected_row_version: rejected.row_version,
            next_status: "APPROVED".to_string(),
            reason: None,
            note: None,
            actor_id: None,
        },
    )
    .await
    .expect_err("REJECTED is terminal");
    assert!(matches!(reopen, AppError::ValidationFailed(_)));
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
            reason_code: Some("seed count".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed stock");

    // The count tolerance is the tenant setting.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE app_settings SET setting_value_json = '2' \
         WHERE setting_key = 'inventory.count_critical_variance' AND setting_scope = 'tenant'"
            .to_string(),
    ))
    .await
    .expect("count tolerance");
    let mut session = controls::create_count_session(
        &db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(location_id),
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
    assert!(
        (session.critical_abs_threshold - 2.0).abs() < f64::EPSILON,
        "the session keeps the setting"
    );
    assert_eq!(line.is_critical, 1, "a variance of 4 is critical at a tolerance of 2");
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
                reason_code: Some("bulk seed".to_string()),
                notes: None,
                source_ref: None,
            },
        )
        .await
        .expect("seed bulk");
    }

    // The reconciliation tolerance is the tenant setting.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE app_settings SET setting_value_json = '0.1' \
         WHERE setting_key = 'inventory.reconciliation_drift_break' AND setting_scope = 'tenant'"
            .to_string(),
    ))
    .await
    .expect("reconciliation tolerance");
    let run = controls::run_reconciliation(&db, RunInventoryReconciliationInput { actor_id: Some(1) })
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

    let drift_run = controls::run_reconciliation(&db, RunInventoryReconciliationInput { actor_id: Some(1) })
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
            reason_code: Some("seed".to_string()),
            notes: None,
            source_ref: None,
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

#[tokio::test]
async fn list_articles_returns_stock_totals_per_article() {
    let db = setup_db().await;
    let stocked_id = create_test_article(&db, "INV-TOT-001").await;
    let empty_id = create_test_article(&db, "INV-TOT-002").await;
    let location_id = main_location_id(&db).await;

    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id: stocked_id,
            location_id,
            delta_qty: 8.0,
            reason_code: Some("ENTREE_ACHAT".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("adjust");
    queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id: stocked_id,
            location_id,
            quantity: 3.0,
            source_type: "WO".to_string(),
            source_id: Some(1),
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("reserve");

    let rows = queries::list_articles(
        &db,
        InventoryArticleFilter {
            search: Some("INV-TOT".to_string()),
        },
    )
    .await
    .expect("list articles");
    let stocked = rows.iter().find(|a| a.id == stocked_id).expect("stocked article");
    assert!((stocked.on_hand_qty - 8.0).abs() < f64::EPSILON);
    assert!((stocked.reserved_qty - 3.0).abs() < f64::EPSILON);
    assert!((stocked.available_qty - 5.0).abs() < f64::EPSILON);

    let empty = rows.iter().find(|a| a.id == empty_id).expect("article without balance");
    assert!(empty.on_hand_qty.abs() < f64::EPSILON);
    assert!(empty.reserved_qty.abs() < f64::EPSILON);
    assert!(empty.available_qty.abs() < f64::EPSILON);
}

#[tokio::test]
async fn work_order_exit_requires_an_existing_work_order_reference() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-WO-EXIT").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 5.0,
            reason_code: Some("ENTREE_ACHAT".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed stock");

    let exit = |source_ref: Option<&str>| InventoryStockAdjustInput {
        article_id,
        location_id,
        delta_qty: -1.0,
        reason_code: Some("SORTIE_OT".to_string()),
        notes: None,
        source_ref: source_ref.map(str::to_string),
    };

    let missing = queries::adjust_stock(&db, exit(None)).await;
    assert!(
        matches!(missing, Err(AppError::ValidationFailed(_))),
        "exit without OT must fail"
    );
    let unknown = queries::adjust_stock(&db, exit(Some("OT-DOES-NOT-EXIST"))).await;
    assert!(
        matches!(unknown, Err(AppError::ValidationFailed(_))),
        "unknown OT must fail"
    );

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_orders (code, type_id, status_id, title, row_version, created_at, updated_at)
         VALUES (?, (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'),
                 (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), 'Pump repair', 1,
                 datetime('now'), datetime('now'))",
        ["OT-TEST-0001".into()],
    ))
    .await
    .expect("insert work order");

    let balance = queries::adjust_stock(&db, exit(Some("OT-TEST-0001")))
        .await
        .expect("exit to an existing OT");
    assert!((balance.on_hand_qty - 4.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn costed_entries_maintain_weighted_average_and_article_stock_value() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-WAC-001").await;
    let location_id = main_location_id(&db).await;
    let entry = |qty: f64| InventoryStockAdjustInput {
        article_id,
        location_id,
        delta_qty: qty,
        reason_code: Some("ENTREE_ACHAT".to_string()),
        notes: None,
        source_ref: None,
    };

    // 10 @ 4.00 then 10 @ 6.00 → average 5.00, value 100.
    queries::adjust_stock_costed(&db, entry(10.0), Some(4.0))
        .await
        .expect("entry 1");
    let balance = queries::adjust_stock_costed(&db, entry(10.0), Some(6.0))
        .await
        .expect("entry 2");
    assert!((balance.moving_avg_unit_cost.expect("avg") - 5.0).abs() < 1e-9);

    // An exit is valued at the average and does not change it.
    let after_exit = queries::adjust_stock_costed(&db, entry(-4.0), None)
        .await
        .expect("exit");
    assert!((after_exit.moving_avg_unit_cost.expect("avg") - 5.0).abs() < 1e-9);
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_cost, extended_cost, cost_source_type FROM inventory_transactions
             WHERE article_id = ? AND movement_type = 'ADJUST_OUT'",
            [article_id.into()],
        ))
        .await
        .expect("query")
        .expect("exit movement");
    let exit_cost: f64 = row.try_get("", "extended_cost").expect("extended_cost");
    assert!((exit_cost - 20.0).abs() < 1e-9);
    let source: String = row.try_get("", "cost_source_type").expect("source");
    assert_eq!(source, "MOVING_AVG");

    // An exit carrying its own cost is refused.
    assert!(matches!(
        queries::adjust_stock_costed(&db, entry(-1.0), Some(3.0)).await,
        Err(AppError::ValidationFailed(_))
    ));
    // A negative cost is refused.
    assert!(matches!(
        queries::adjust_stock_costed(&db, entry(1.0), Some(-3.0)).await,
        Err(AppError::ValidationFailed(_))
    ));

    let articles = queries::list_articles(
        &db,
        InventoryArticleFilter {
            search: Some("INV-WAC".to_string()),
        },
    )
    .await
    .expect("list");
    let article = articles.iter().find(|a| a.id == article_id).expect("article");
    assert!((article.avg_unit_cost.expect("avg") - 5.0).abs() < 1e-9);
    assert!((article.stock_value.expect("value") - 80.0).abs() < 1e-9);
    assert!(article.unvalued_qty.abs() < f64::EPSILON);
}

#[test]
fn weighted_moving_average_blends_by_quantity() {
    use crate::inventory::valuation::weighted_moving_average;
    assert!((weighted_moving_average(10.0, Some(4.0), 10.0, 6.0) - 5.0).abs() < 1e-9);
    // First costed receipt on uncosted stock takes the receipt price.
    assert!((weighted_moving_average(5.0, None, 5.0, 8.0) - 8.0).abs() < 1e-9);
    assert!((weighted_moving_average(0.0, Some(3.0), 2.0, 7.0) - 7.0).abs() < 1e-9);
}

/// One low-stock definition (`evaluate_reorder`): the dashboard tiles, the alerts and the
/// article list agree, and an article with no stock row at all (never received) counts.
#[tokio::test]
async fn low_stock_is_one_definition_across_dashboard_alerts_and_list() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "LOW-ZERO-001").await; // reorder point 5, no stock
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET is_critical_spare = 1 WHERE id = ?",
        [article_id.into()],
    ))
    .await
    .expect("critical spare");

    let triggers = queries::reorder_triggers_by_article(&db).await.expect("triggers");
    assert_eq!(
        triggers.get(&article_id).map(String::as_str),
        Some("reorder_point"),
        "a never-stocked article is at or below its reorder point"
    );

    let summary = queries::get_procurement_dashboard_summary(&db).await.expect("summary");
    assert_eq!(summary.low_stock_articles, i64::try_from(triggers.len()).unwrap());
    assert!(summary.critical_low_stock_articles >= 1);
    assert!(summary.open_requisition_statuses.contains(&"SUBMITTED".to_string()));
    assert!(summary.late_po_statuses.contains(&"APPROVED".to_string()));

    let listed = queries::list_articles(
        &db,
        InventoryArticleFilter {
            search: Some("LOW-ZERO-001".into()),
        },
    )
    .await
    .expect("list");
    assert_eq!(listed[0].reorder_trigger.as_deref(), Some("reorder_point"));

    let alerts = queries::get_procurement_alerts(&db).await.expect("alerts");
    let critical = alerts
        .iter()
        .find(|a| a.kind == "CRITICAL_STOCK" && a.entity_id == Some(article_id))
        .expect("critical stock alert for the never-stocked spare");
    assert_eq!(critical.entity_code.as_deref(), Some("LOW-ZERO-001"));
    assert_eq!(critical.params.get("threshold_qty").map(String::as_str), Some("5"));
    assert_eq!(critical.params.get("available_qty").map(String::as_str), Some("0"));
}

/// "To reorder" is counted per article on the home page and the analytics overview — the article
/// list's "below reorder point" filter — never per article × store line.
#[tokio::test]
async fn to_reorder_counts_articles_like_the_article_list() {
    let db = setup_db().await;
    let a = create_test_article(&db, "RO-ART-A").await; // reorder point 5, no stock
    let b = create_test_article(&db, "RO-ART-B").await;
    // A second active store: each never-stocked article is short in both.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO warehouses (entity_sync_id, code, name, is_active, row_version) \
         VALUES ('ro-wh-2', 'RO-WH-2', 'Second store', 1, 1)"
            .to_string(),
    ))
    .await
    .expect("second store");

    let listed = queries::list_articles(&db, InventoryArticleFilter::default())
        .await
        .expect("list");
    let list_count = listed
        .iter()
        .filter(|x| x.is_active == 1 && x.reorder_trigger.is_some())
        .count() as i64;
    assert!(listed.iter().any(|x| x.id == a && x.reorder_trigger.is_some()));
    assert!(listed.iter().any(|x| x.id == b && x.reorder_trigger.is_some()));

    let home = crate::home::supervision::stock(&db).await.expect("home stock");
    assert_eq!(home.articles_to_reorder, list_count, "home headline = article list");
    assert!(
        home.lines_to_reorder > home.articles_to_reorder,
        "an article short in two stores is two lines but one article"
    );
    let per_article = queries::reorder_triggers_by_article(&db).await.expect("triggers");
    assert_eq!(
        i64::try_from(per_article.len()).unwrap(),
        list_count,
        "analytics overview count"
    );
}

/// "Low stock only" on the stock balances is the shared low-stock rule (`evaluate_reorder`, per
/// article and store), not a per-location comparison: two bins of one store that are each under the
/// reorder point but together above it are not low; an article short only of its minimum stock is.
#[tokio::test]
async fn low_stock_only_follows_the_shared_rule() {
    let db = setup_db().await;
    let main = main_location_id(&db).await;
    let warehouse: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT warehouse_id FROM stock_locations WHERE id = ?",
            [main.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "warehouse_id")
        .expect("warehouse");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_locations (warehouse_id, code, name, is_active) VALUES (?, 'LS-BIN2', 'Bin 2', 1)",
        [warehouse.into()],
    ))
    .await
    .expect("second bin");
    let bin2: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM stock_locations WHERE code = 'LS-BIN2'".to_string(),
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "id")
        .expect("bin2");

    // Reorder point 5: 3 + 3 in the store = 6, above it (the old per-bin rule listed both bins).
    let split = create_test_article(&db, "LS-SPLIT").await;
    // Reorder point 0, minimum stock 1, nothing in stock: short of its minimum.
    let min_only = create_test_article(&db, "LS-MIN").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET reorder_point = 0 WHERE id = ?",
        [min_only.into()],
    ))
    .await
    .expect("no reorder point");
    for loc in [main, bin2] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty) \
             VALUES (?, ?, ?, 3, 0, 3)",
            [split.into(), warehouse.into(), loc.into()],
        ))
        .await
        .expect("balance");
    }

    let low = queries::list_stock_balances(
        &db,
        InventoryStockFilter {
            article_id: None,
            warehouse_id: Some(warehouse),
            low_stock_only: Some(true),
        },
    )
    .await
    .expect("low stock");
    assert!(low.iter().all(|b| b.article_id != split), "6 in the store is above 5");
    assert!(
        low.iter().any(|b| b.article_id == min_only),
        "short of its minimum stock"
    );
    let shared: std::collections::HashSet<(i64, i64)> = queries::evaluate_reorder(&db, Some(warehouse))
        .await
        .expect("reorder")
        .into_iter()
        .map(|r| (r.article_id, r.warehouse_id))
        .collect();
    assert!(
        low.iter().all(|b| shared.contains(&(b.article_id, b.warehouse_id))),
        "every listed row is low by the shared rule"
    );
}

/// The count and the reconciliation read their tolerance from the tenant settings (seeded 5 and
/// 0.01); without the setting they refuse rather than guess.
#[tokio::test]
async fn count_tolerances_come_from_the_tenant_settings() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-CC-TOL").await;
    let location_id = main_location_id(&db).await;
    queries::adjust_stock(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: 10.0,
            reason_code: Some("seed count".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed stock");
    let session = controls::create_count_session(
        &db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(location_id),
            actor_id: Some(1),
        },
    )
    .await
    .expect("create session");
    assert!((session.critical_abs_threshold - 5.0).abs() < f64::EPSILON, "seeded 5");
    let session = controls::transition_count_session(
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
    assert_eq!(line.is_critical, 0, "a variance of 4 is under the seeded 5");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM app_settings WHERE setting_key IN          ('inventory.count_critical_variance', 'inventory.reconciliation_drift_break')"
            .to_string(),
    ))
    .await
    .expect("remove settings");
    assert!(controls::create_count_session(
        &db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(location_id),
            actor_id: Some(1),
        },
    )
    .await
    .is_err());
    assert!(
        controls::run_reconciliation(&db, RunInventoryReconciliationInput { actor_id: Some(1) })
            .await
            .is_err()
    );
}

async fn insert_aux_location(db: &DatabaseConnection, code: &str) -> i64 {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_locations (entity_sync_id, warehouse_id, code, name, is_default, is_active)
         SELECT ?, warehouse_id, ?, 'Aux bin', 0, 1
         FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1",
        [crate::sync::identity::new_entity_sync_id().into(), code.into()],
    ))
    .await
    .expect("insert aux location");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM stock_locations WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await
        .expect("query aux")
        .expect("aux row");
    row.try_get("", "id").expect("aux id")
}

async fn seed_stock(db: &DatabaseConnection, article_id: i64, location_id: i64, qty: f64) {
    queries::adjust_stock(
        db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: qty,
            reason_code: Some("seed transfer".to_string()),
            notes: None,
            source_ref: None,
        },
    )
    .await
    .expect("seed stock");
}

fn transfer_input(article_id: i64, from: i64, to: i64, quantity: f64, notes: Option<&str>) -> InventoryTransferInput {
    InventoryTransferInput {
        article_id,
        from_location_id: from,
        to_location_id: to,
        quantity,
        source_type: None,
        source_id: None,
        source_ref: None,
        notes: notes.map(str::to_string),
        lot_id: None,
    }
}

fn transfer_refusal_of(err: AppError) -> (String, std::collections::HashMap<String, String>) {
    match err {
        AppError::OrgValidationFailed(issues) => {
            let issue = issues.into_iter().next().expect("one issue");
            (issue.code, issue.params)
        }
        other => panic!("expected a coded refusal, got {other:?}"),
    }
}

async fn transfer_movement_count(db: &DatabaseConnection, article_id: i64) -> i64 {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM inventory_transactions
             WHERE article_id = ? AND movement_type IN ('TRANSFER_OUT','TRANSFER_IN')",
            [article_id.into()],
        ))
        .await
        .expect("count movements")
        .expect("row");
    row.try_get("", "n").expect("n")
}

#[tokio::test]
async fn transfer_refusals_carry_stable_codes_and_change_nothing() {
    use crate::inventory::queries::transfer_codes as codes;
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER-CODES").await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-CODES").await;
    seed_stock(&db, article_id, main_loc, 8.0).await;
    queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id: main_loc,
            quantity: 3.0,
            source_type: "WO".to_string(),
            source_id: Some(9101),
            source_ref: Some("WO-9101".to_string()),
            notes: None,
        },
    )
    .await
    .expect("reserve");

    let note = Some("réappro atelier");
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, main_loc, aux_loc, 0.0, note), None)
            .await
            .expect_err("zero quantity"),
    );
    assert_eq!(code, codes::QUANTITY_INVALID);
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, main_loc, aux_loc, -2.0, note), None)
            .await
            .expect_err("negative quantity"),
    );
    assert_eq!(code, codes::QUANTITY_INVALID);
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, main_loc, main_loc, 1.0, note), None)
            .await
            .expect_err("same location"),
    );
    assert_eq!(code, codes::SAME_LOCATION);

    for bad_note in [None, Some(""), Some("    "), Some("  ab  "), Some("abcd")] {
        let (code, params) = transfer_refusal_of(
            queries::transfer_stock(&db, transfer_input(article_id, main_loc, aux_loc, 1.0, bad_note), None)
                .await
                .expect_err("missing or short note"),
        );
        assert_eq!(code, codes::NOTE_TOO_SHORT, "note {bad_note:?}");
        assert_eq!(params.get("min").map(String::as_str), Some("5"));
    }

    // 8 on hand, 3 reserved: 5 available. 6 is refused even though 8 are on hand.
    let (code, params) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, main_loc, aux_loc, 6.0, note), None)
            .await
            .expect_err("reserved stock is not transferable"),
    );
    assert_eq!(code, codes::INSUFFICIENT_AVAILABLE);
    assert_eq!(params.get("available").map(String::as_str), Some("5"));
    assert_eq!(params.get("requested").map(String::as_str), Some("6"));

    let (code, params) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, main_loc, 987_654, 1.0, note), None)
            .await
            .expect_err("unknown destination"),
    );
    assert_eq!(code, codes::LOCATION_UNAVAILABLE);
    assert_eq!(params.get("side").map(String::as_str), Some("to"));
    let (code, params) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, 987_654, aux_loc, 1.0, note), None)
            .await
            .expect_err("unknown source"),
    );
    assert_eq!(code, codes::LOCATION_UNAVAILABLE);
    assert_eq!(params.get("side").map(String::as_str), Some("from"));

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET is_active = 0 WHERE id = ?",
        [article_id.into()],
    ))
    .await
    .expect("deactivate");
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(article_id, main_loc, aux_loc, 1.0, note), None)
            .await
            .expect_err("inactive article"),
    );
    assert_eq!(code, codes::ARTICLE_INACTIVE);

    assert_eq!(transfer_movement_count(&db, article_id).await, 0);
    let balances = queries::list_stock_balances(&db, InventoryStockFilter::default())
        .await
        .expect("balances");
    let main_balance = balances
        .iter()
        .find(|b| b.article_id == article_id && b.location_id == main_loc)
        .expect("source balance");
    assert!((main_balance.on_hand_qty - 8.0).abs() < 1e-9);
    assert!((main_balance.reserved_qty - 3.0).abs() < 1e-9);
}

#[tokio::test]
async fn transfer_of_tracked_articles_refuses_with_codes() {
    use crate::inventory::queries::transfer_codes as codes;
    let db = setup_db().await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-TRACKED").await;
    let note = Some("réappro atelier");

    let serial_id = create_test_article(&db, "INV-XFER-SERIAL").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET requires_serial_tracking = 1 WHERE id = ?",
        [serial_id.into()],
    ))
    .await
    .expect("serial tracking");
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(serial_id, main_loc, aux_loc, 1.5, note), None)
            .await
            .expect_err("fractional serial quantity"),
    );
    assert_eq!(code, codes::WHOLE_UNITS);

    // A batch-tracked article without any lot in stock: FEFO cannot cover the quantity, and a
    // lot that does not exist is refused the same way.
    let batch_id = create_test_article(&db, "INV-XFER-BATCH").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET requires_batch_tracking = 1 WHERE id = ?",
        [batch_id.into()],
    ))
    .await
    .expect("batch tracking");
    // Plain on-hand stock (not in a lot) is enough for the available check, not for the lots.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty)
         SELECT ?, warehouse_id, id, 5, 0, 5 FROM stock_locations WHERE id = ?",
        [batch_id.into(), main_loc.into()],
    ))
    .await
    .expect("balance without lot");
    let (code, params) = transfer_refusal_of(
        queries::transfer_stock(&db, transfer_input(batch_id, main_loc, aux_loc, 2.0, note), None)
            .await
            .expect_err("no lot in stock"),
    );
    assert_eq!(code, codes::LOT_UNAVAILABLE);
    assert_eq!(params.get("requested").map(String::as_str), Some("2"));
    let mut with_lot = transfer_input(batch_id, main_loc, aux_loc, 2.0, note);
    with_lot.lot_id = Some(987_654);
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(&db, with_lot, None)
            .await
            .expect_err("unknown lot"),
    );
    assert_eq!(code, codes::LOT_UNAVAILABLE);
    assert_eq!(transfer_movement_count(&db, batch_id).await, 0);
    // Refused before the first write: balances are exactly as they were.
    assert_eq!(balance_of(&db, batch_id, main_loc).await, Some((5.0, 0.0)));
    assert_eq!(balance_of(&db, batch_id, aux_loc).await, None);
}

#[tokio::test]
async fn transfer_moves_available_stock_and_records_who_when_and_why_on_both_movements() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER-OK").await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-OK").await;
    seed_stock(&db, article_id, main_loc, 8.0).await;
    queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id: main_loc,
            quantity: 3.0,
            source_type: "WO".to_string(),
            source_id: Some(9102),
            source_ref: Some("WO-9102".to_string()),
            notes: None,
        },
    )
    .await
    .expect("reserve");
    let actor: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts ORDER BY id LIMIT 1".to_string(),
        ))
        .await
        .expect("query user")
        .expect("a user")
        .try_get("", "id")
        .expect("user id");

    // The note is trimmed; exactly the available quantity (5) can leave.
    let balances = queries::transfer_stock(
        &db,
        transfer_input(article_id, main_loc, aux_loc, 5.0, Some("  Renfort magasin atelier  ")),
        Some(actor),
    )
    .await
    .expect("transfer");
    assert_eq!(balances.len(), 2);
    assert!(
        (balances[0].on_hand_qty - 3.0).abs() < 1e-9,
        "source keeps the reserved 3"
    );
    assert!((balances[0].reserved_qty - 3.0).abs() < 1e-9);
    assert!(balances[0].available_qty.abs() < 1e-9);
    assert!((balances[1].on_hand_qty - 5.0).abs() < 1e-9);
    assert!(balances[1].reserved_qty.abs() < 1e-9);

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT movement_type, quantity, reason, performed_by_id, performed_at, location_id
             FROM inventory_transactions
             WHERE article_id = ? AND movement_type IN ('TRANSFER_OUT','TRANSFER_IN')
             ORDER BY id",
            [article_id.into()],
        ))
        .await
        .expect("movements");
    assert_eq!(rows.len(), 2, "one OUT and one IN");
    for (row, (kind, location)) in rows.iter().zip([("TRANSFER_OUT", main_loc), ("TRANSFER_IN", aux_loc)]) {
        let movement_type: String = row.try_get("", "movement_type").expect("type");
        assert_eq!(movement_type, kind);
        let quantity: f64 = row.try_get("", "quantity").expect("qty");
        assert!((quantity - 5.0).abs() < 1e-9);
        let reason: Option<String> = row.try_get("", "reason").expect("reason");
        assert_eq!(reason.as_deref(), Some("Renfort magasin atelier"));
        let by: Option<i64> = row.try_get("", "performed_by_id").expect("by");
        assert_eq!(by, Some(actor));
        let at: Option<String> = row.try_get("", "performed_at").expect("at");
        assert!(at.is_some_and(|v| !v.is_empty()));
        let loc: i64 = row.try_get("", "location_id").expect("loc");
        assert_eq!(loc, location);
    }
}

async fn balance_of(db: &DatabaseConnection, article_id: i64, location_id: i64) -> Option<(f64, f64)> {
    let balances = queries::list_stock_balances(
        db,
        InventoryStockFilter {
            article_id: Some(article_id),
            ..Default::default()
        },
    )
    .await
    .expect("balances");
    balances
        .into_iter()
        .find(|b| b.location_id == location_id)
        .map(|b| (b.on_hand_qty, b.reserved_qty))
}

#[tokio::test]
async fn transfer_of_the_displayed_available_quantity_is_never_refused_by_float_rounding() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER-FLOAT").await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-FLOAT").await;
    seed_stock(&db, article_id, main_loc, 0.3).await;
    queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id: main_loc,
            quantity: 0.1,
            source_type: "WO".to_string(),
            source_id: Some(9103),
            source_ref: Some("WO-9103".to_string()),
            notes: None,
        },
    )
    .await
    .expect("reserve");

    // 0.3 on hand, 0.1 reserved: 0.2 is shown as available and must go through.
    let balances = queries::transfer_stock(
        &db,
        transfer_input(article_id, main_loc, aux_loc, 0.2, Some("Renfort magasin atelier")),
        None,
    )
    .await
    .expect("0.2 is available");
    assert!((balances[0].on_hand_qty - 0.1).abs() < 1e-9);
    assert!(balances[0].available_qty.abs() < 1e-9);
    assert!((balances[1].on_hand_qty - 0.2).abs() < 1e-9);

    // Nothing is left to transfer: a coded refusal, never an uncoded one.
    let (code, _) = transfer_refusal_of(
        queries::transfer_stock(
            &db,
            transfer_input(article_id, main_loc, aux_loc, 0.05, Some("Renfort magasin atelier")),
            None,
        )
        .await
        .expect_err("reserved stock stays"),
    );
    assert_eq!(code, "INV_TRANSFER_INSUFFICIENT_AVAILABLE");
}

#[tokio::test]
async fn transfer_of_a_lot_tracked_article_moves_the_chosen_lot_and_records_it_on_both_movements() {
    use crate::inventory::domain::NewLotInput;
    use crate::inventory::queries::AdjustStockOptions;
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER-LOT").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE articles SET requires_batch_tracking = 1 WHERE id = ?",
        [article_id.into()],
    ))
    .await
    .expect("batch tracking");
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-LOT").await;
    queries::adjust_stock_full(
        &db,
        InventoryStockAdjustInput {
            article_id,
            location_id: main_loc,
            delta_qty: 6.0,
            reason_code: Some("seed transfer".to_string()),
            notes: None,
            source_ref: None,
        },
        AdjustStockOptions {
            unit_cost: Some(2.0),
            new_lot: Some(NewLotInput {
                lot_number: Some("LOT-A".to_string()),
                serial_number: None,
                expiry_date: None,
            }),
            ..Default::default()
        },
    )
    .await
    .expect("lot entry");
    let lot_id = crate::inventory::lots::list_lot_stock(&db, article_id, Some(main_loc))
        .await
        .expect("lots")
        .first()
        .expect("one lot")
        .lot_id;
    let actor: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts ORDER BY id LIMIT 1".to_string(),
        ))
        .await
        .expect("query user")
        .expect("a user")
        .try_get("", "id")
        .expect("user id");

    let mut input = transfer_input(article_id, main_loc, aux_loc, 4.0, Some("Renfort magasin atelier"));
    input.lot_id = Some(lot_id);
    let balances = queries::transfer_stock(&db, input, Some(actor))
        .await
        .expect("lot transfer");
    assert!((balances[0].on_hand_qty - 2.0).abs() < 1e-9);
    assert!((balances[1].on_hand_qty - 4.0).abs() < 1e-9);

    // The lot ledger follows the parts.
    assert!(
        (crate::inventory::lots::lot_stock_at(&db, lot_id, main_loc)
            .await
            .expect("src")
            - 2.0)
            .abs()
            < 1e-9
    );
    assert!(
        (crate::inventory::lots::lot_stock_at(&db, lot_id, aux_loc)
            .await
            .expect("dst")
            - 4.0)
            .abs()
            < 1e-9
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT movement_type, quantity, reason, performed_by_id, lot_id
             FROM inventory_transactions
             WHERE article_id = ? AND movement_type IN ('TRANSFER_OUT','TRANSFER_IN')
             ORDER BY id",
            [article_id.into()],
        ))
        .await
        .expect("movements");
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let quantity: f64 = row.try_get("", "quantity").expect("qty");
        assert!((quantity - 4.0).abs() < 1e-9);
        let reason: Option<String> = row.try_get("", "reason").expect("reason");
        assert_eq!(reason.as_deref(), Some("Renfort magasin atelier"));
        let by: Option<i64> = row.try_get("", "performed_by_id").expect("by");
        assert_eq!(by, Some(actor));
        let lot: Option<i64> = row.try_get("", "lot_id").expect("lot");
        assert_eq!(lot, Some(lot_id));
    }
}

#[tokio::test]
async fn a_database_failure_during_a_transfer_is_not_presented_as_a_business_refusal() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER-DBERR").await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-DBERR").await;
    seed_stock(&db, article_id, main_loc, 3.0).await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "ALTER TABLE stock_locations RENAME TO stock_locations_gone".to_string(),
    ))
    .await
    .expect("break the stores table");
    let err = queries::transfer_stock(
        &db,
        transfer_input(article_id, main_loc, aux_loc, 1.0, Some("Renfort magasin atelier")),
        None,
    )
    .await
    .expect_err("broken database");
    assert!(
        !matches!(err, AppError::OrgValidationFailed(_) | AppError::ValidationFailed(_)),
        "a real database error must propagate, got {err:?}"
    );
}

#[tokio::test]
async fn a_transfer_never_rewrites_the_reserved_quantity_and_reservations_stay_releasable() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-XFER-RSV").await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-RSV").await;
    seed_stock(&db, article_id, main_loc, 5.0).await;
    let mut reservations = Vec::new();
    for (qty, wo) in [(0.1, 9201_i64), (0.2, 9202)] {
        reservations.push(
            queries::reserve_stock(
                &db,
                InventoryReserveInput {
                    article_id,
                    location_id: main_loc,
                    quantity: qty,
                    source_type: "WO".to_string(),
                    source_id: Some(wo),
                    source_ref: Some(format!("WO-{wo}")),
                    notes: None,
                },
            )
            .await
            .expect("reserve"),
        );
    }
    // 0.1 + 0.2 is stored as 0.30000000000000004: the exact bits must survive a transfer.
    let (_, reserved_before) = balance_of(&db, article_id, main_loc).await.expect("balance");
    assert_eq!(reserved_before.to_bits(), (0.1_f64 + 0.2_f64).to_bits());

    queries::transfer_stock(
        &db,
        transfer_input(article_id, main_loc, aux_loc, 2.0, Some("Renfort magasin atelier")),
        None,
    )
    .await
    .expect("free stock transfers");
    let (on_hand_after, reserved_after) = balance_of(&db, article_id, main_loc).await.expect("balance");
    assert_eq!(
        reserved_after.to_bits(),
        reserved_before.to_bits(),
        "reserved is untouched"
    );
    assert_eq!(
        on_hand_after.to_bits(),
        (5.0_f64 - 2.0_f64).to_bits(),
        "on hand is raw arithmetic"
    );

    // The movement history still adds up to the balance (no snapping drift).
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE movement_type WHEN 'ADJUST_IN' THEN quantity
                                                    WHEN 'TRANSFER_OUT' THEN -quantity ELSE 0 END), 0) AS qty
             FROM inventory_transactions WHERE article_id = ? AND location_id = ?",
            [article_id.into(), main_loc.into()],
        ))
        .await
        .expect("sum")
        .expect("row");
    let history: f64 = row.try_get("", "qty").expect("qty");
    assert_eq!(history.to_bits(), on_hand_after.to_bits());

    // Both reservations can still be released, in the opposite order of their creation.
    for reservation in reservations.iter().rev() {
        queries::release_stock_reservation(
            &db,
            InventoryReleaseReservationInput {
                reservation_id: reservation.id,
                notes: None,
            },
        )
        .await
        .expect("release");
    }
    let (_, reserved_end) = balance_of(&db, article_id, main_loc).await.expect("balance");
    assert!(reserved_end.abs() < 1e-12, "nothing stays reserved, got {reserved_end}");
}
