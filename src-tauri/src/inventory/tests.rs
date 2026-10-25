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

    // The four-eyes tolerance is the plant setting (here 20 % of the system quantity).
    set_tenant_setting(&db, "inventory.count_four_eyes_percent_threshold", "20").await;
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
            expected_row_version: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("upsert line");
    assert_eq!(
        session.critical_abs_threshold, None,
        "a new session records no tolerance"
    );
    assert_eq!(line.is_critical, 1, "a variance of 4 on 10 (40 %) is above 20 %");
    assert_eq!(line.counted_by_id, Some(1), "the counter is recorded");
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
    // AU-70: the count is not approved while a variance waits for its reviewer.
    let not_approved = controls::transition_count_session(
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
    .expect_err("a variance without its reviewer's approval");
    match not_approved {
        AppError::OrgValidationFailed(issues) => {
            assert_eq!(issues[0].code, controls::REFUSED_COUNT_LINE_NOT_APPROVED);
            assert_eq!(issues[0].params.get("lines").map(String::as_str), Some("1"));
        }
        other => panic!("expected COUNT_LINE_NOT_APPROVED, got {other:?}"),
    }

    // A session that reached `approved` some other way (for example received from a PC of an older
    // version) still cannot post a variance without its reviewer's evidence.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_sessions SET status = 'approved' WHERE id = ?",
        [session.id.into()],
    ))
    .await
    .expect("approved without the line approval");
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
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_sessions SET status = 'submitted' WHERE id = ?",
        [session.id.into()],
    ))
    .await
    .expect("back to submitted");

    let _approved_line = controls::approve_count_line(
        &db,
        ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: count_reviewer(&db).await,
            reviewer_evidence: "Double-check done with signed sheet".to_string(),
        },
    )
    .await
    .expect("approve line");
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
    .expect("approve once the variance is approved");
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

/// AU-70: `posted` and `reversed` are reached only by the posting and the reversal (the stock moves
/// with them); a status change to either is refused and leaves the session and the stock as they
/// were.
#[tokio::test]
async fn a_count_session_is_never_posted_or_reversed_by_a_status_change() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-CC-AU70").await;
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
    let move_to =
        |session: &crate::inventory::domain::InventoryCountSession, next: &str| TransitionInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            next_status: next.to_string(),
            reason: None,
            actor_id: Some(1),
        };
    let refused_code = |err: AppError| match err {
        AppError::OrgValidationFailed(issues) => issues[0].code.clone(),
        other => format!("{other:?}"),
    };
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
    session = controls::transition_count_session(&db, move_to(&session, "counting"))
        .await
        .expect("counting");
    let line = controls::upsert_count_line(
        &db,
        UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id,
            location_id,
            counted_qty: 7.0,
            variance_reason_code: Some("COUNT_ERROR".to_string()),
            expected_row_version: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("line");
    session = controls::transition_count_session(&db, move_to(&session, "submitted"))
        .await
        .expect("submitted");
    controls::approve_count_line(
        &db,
        ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: count_reviewer(&db).await,
            reviewer_evidence: "Recounted with the store keeper".to_string(),
        },
    )
    .await
    .expect("approve line");
    session = controls::transition_count_session(&db, move_to(&session, "approved"))
        .await
        .expect("approved");

    let posted_by_status = controls::transition_count_session(&db, move_to(&session, "posted"))
        .await
        .expect_err("posted only by the posting");
    assert_eq!(
        refused_code(posted_by_status),
        controls::REFUSED_COUNT_STATUS_BY_POSTING_ONLY
    );
    let (on_hand, _) = balance_of(&db, article_id, location_id).await.expect("balance");
    assert!((on_hand - 10.0).abs() < 1e-9, "the stock did not move");
    let unchanged = controls::list_count_sessions(&db)
        .await
        .expect("sessions")
        .into_iter()
        .find(|s| s.id == session.id)
        .expect("session");
    assert_eq!(unchanged.status, "approved");

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
    let (on_hand, _) = balance_of(&db, article_id, location_id).await.expect("balance");
    assert!((on_hand - 7.0).abs() < 1e-9, "posted: 10 - 3");

    let reversed_by_status = controls::transition_count_session(&db, move_to(&posted, "reversed"))
        .await
        .expect_err("reversed only by the reversal");
    assert_eq!(
        refused_code(reversed_by_status),
        controls::REFUSED_COUNT_STATUS_BY_POSTING_ONLY
    );
    let (on_hand, _) = balance_of(&db, article_id, location_id).await.expect("balance");
    assert!((on_hand - 7.0).abs() < 1e-9, "the stock did not move back");
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

/// The count's four-eyes tolerance comes from the plant settings, judged at entry (blank = strict,
/// never zero); the reconciliation reads its seeded tolerance and refuses without it.
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
    assert_eq!(session.critical_abs_threshold, None);
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
            expected_row_version: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("upsert line");
    assert_eq!(line.is_critical, 1, "both thresholds blank: every variance is above");
    let enter = |counted_qty: f64, line: &crate::inventory::domain::InventoryCountLine| UpsertInventoryCountLineInput {
        session_id: session.id,
        article_id,
        location_id,
        counted_qty,
        variance_reason_code: Some("COUNT_ERROR".to_string()),
        expected_row_version: Some(line.row_version),
        actor_id: Some(1),
    };
    // 50 % only: a variance of 4 on 10 is within it; exactly 50 % is within it too (">").
    set_tenant_setting(&db, "inventory.count_four_eyes_percent_threshold", "50").await;
    let line = controls::upsert_count_line(&db, enter(6.0, &line)).await.expect("40 %");
    assert_eq!(line.is_critical, 0);
    let line = controls::upsert_count_line(&db, enter(5.0, &line)).await.expect("50 %");
    assert_eq!(line.is_critical, 0, "exactly at the threshold: within");
    let line = controls::upsert_count_line(&db, enter(4.0, &line)).await.expect("60 %");
    assert_eq!(line.is_critical, 1);
    // A value threshold set but no unit cost at the location: the percent alone judges.
    set_tenant_setting(&db, "inventory.count_four_eyes_value_threshold", "0").await;
    let line = controls::upsert_count_line(&db, enter(6.0, &line))
        .await
        .expect("no unit cost");
    assert_eq!(
        line.is_critical, 0,
        "no unit cost: value threshold ignored, 40 % within 50 %"
    );
    // With a unit cost, any threshold exceeded is enough: 4 × 3 = 12 > 0.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_balances SET moving_avg_unit_cost = 3 WHERE article_id = ? AND location_id = ?",
        [article_id.into(), location_id.into()],
    ))
    .await
    .expect("unit cost");
    let line = controls::upsert_count_line(&db, enter(6.0, &line))
        .await
        .expect("valued");
    assert_eq!(line.is_critical, 1, "the value threshold is exceeded");
    let entered = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT details_json FROM audit_events WHERE action_code = 'inventory.count.line_entered' \
             ORDER BY id DESC LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("audit")
        .expect("the entry is audited")
        .try_get::<String>("", "details_json")
        .expect("details");
    let entered: serde_json::Value = serde_json::from_str(&entered).expect("json");
    assert_eq!(
        entered["four_eyes_tolerance"]["value_threshold"],
        serde_json::json!(0.0)
    );
    assert_eq!(
        entered["four_eyes_tolerance"]["percent_threshold"],
        serde_json::json!(50.0)
    );
    assert_eq!(entered["four_eyes_tolerance"]["unit_cost"], serde_json::json!(3.0));

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM app_settings WHERE setting_key = 'inventory.reconciliation_drift_break'".to_string(),
    ))
    .await
    .expect("remove settings");
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

#[tokio::test]
async fn reconciliation_of_a_consistent_ledger_reports_no_drift_and_still_catches_a_real_one() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-RECON-OK").await;
    let main_loc = main_location_id(&db).await;
    let aux_loc = insert_aux_location(&db, "AUX-RECON").await;

    let adjust = |delta: f64| InventoryStockAdjustInput {
        article_id,
        location_id: main_loc,
        delta_qty: delta,
        reason_code: Some("recon ledger".to_string()),
        notes: None,
        source_ref: None,
    };
    // ADJUST_IN then ADJUST_OUT.
    queries::adjust_stock(&db, adjust(20.0)).await.expect("adjust in");
    queries::adjust_stock(&db, adjust(-3.0)).await.expect("adjust out");
    // A transfer (TRANSFER_OUT + TRANSFER_IN).
    queries::transfer_stock(
        &db,
        transfer_input(article_id, main_loc, aux_loc, 4.0, Some("recon transfer")),
        Some(1),
    )
    .await
    .expect("transfer");
    // Reserve, issue, return (RESERVE, ISSUE, RETURN).
    let reservation = queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id: main_loc,
            quantity: 5.0,
            source_type: "WO".to_string(),
            source_id: Some(9301),
            source_ref: Some("WO-9301".to_string()),
            notes: None,
        },
    )
    .await
    .expect("reserve");
    queries::issue_reserved_stock(
        &db,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity: 3.0,
            source_type: Some("WO".to_string()),
            source_id: Some(9301),
            source_ref: Some("WO-9301".to_string()),
            notes: None,
        },
    )
    .await
    .expect("issue");
    queries::return_reserved_stock(
        &db,
        InventoryReturnInput {
            reservation_id: reservation.id,
            quantity: 1.0,
            notes: None,
        },
    )
    .await
    .expect("return");

    // A posted count adjusts the stock (COUNT_ADJUST).
    let (on_hand, _) = balance_of(&db, article_id, main_loc)
        .await
        .expect("balance before count");
    let mut session = controls::create_count_session(
        &db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(main_loc),
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
            location_id: main_loc,
            counted_qty: on_hand - 2.0,
            variance_reason_code: Some("COUNT_ERROR".to_string()),
            expected_row_version: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("count line");
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
    .expect("submitted");
    // The variance is approved before the count is (AU-70).
    controls::approve_count_line(
        &db,
        ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: count_reviewer(&db).await,
            reviewer_evidence: "Counted twice".to_string(),
        },
    )
    .await
    .expect("approve line");
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
    .expect("approved");
    controls::post_count_session(
        &db,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(1),
        },
    )
    .await
    .expect("post count");

    let consistent = controls::run_reconciliation(&db, RunInventoryReconciliationInput { actor_id: Some(1) })
        .await
        .expect("reconciliation of a consistent ledger");
    assert!(consistent.checked_rows >= 2, "both locations are checked");
    let findings = controls::list_reconciliation_findings(&db, consistent.id)
        .await
        .expect("findings");
    assert_eq!(
        consistent.drift_rows, 0,
        "a ledger made only of real commands must not drift: {findings:?}"
    );

    // A balance edited by hand is a real drift and is still reported.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "UPDATE stock_balances SET on_hand_qty = on_hand_qty + 7, available_qty = available_qty + 7 \
             WHERE article_id = {article_id} AND location_id = {main_loc}"
        ),
    ))
    .await
    .expect("edit balance by hand");
    let drifted = controls::run_reconciliation(&db, RunInventoryReconciliationInput { actor_id: Some(1) })
        .await
        .expect("drift run");
    assert_eq!(drifted.drift_rows, 1);
    let findings = controls::list_reconciliation_findings(&db, drifted.id)
        .await
        .expect("drift findings");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].location_id, main_loc);
    assert!((findings[0].drift_qty - 7.0).abs() < 1e-9);
}

/// Run a repairable order through the real procurement path up to `last` and return its movements.
async fn repairable_to_write_off(db: &DatabaseConnection, article_id: i64, location_id: i64, steps: &[&str]) {
    let order = procurement::create_repairable_order(
        db,
        CreateRepairableOrderInput {
            article_id,
            quantity: 2.0,
            source_location_id: location_id,
            return_location_id: Some(location_id),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("ledger write-off".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("create");
    let mut row_version = order.row_version;
    for step in steps {
        let next = procurement::transition_repairable_order(
            db,
            TransitionRepairableOrderInput {
                next_status: (*step).to_string(),
                reason: Some("ledger write-off".to_string()),
                ..repairable_transition(order.id, row_version, location_id)
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{step}: {e:?}"));
        row_version = next.row_version;
    }
}

async fn assert_repairable_write_off_reconciles(steps: &[&str], code: &str, movement: &str, sign: f64) {
    let db = setup_db().await;
    let article_id = create_test_article(&db, code).await;
    let location_id = main_location_id(&db).await;
    seed_stock(&db, article_id, location_id, 5.0).await;
    repairable_to_write_off(&db, article_id, location_id, steps).await;
    let movements = repairable_movements(&db, article_id).await;
    assert!(
        movements.iter().any(|t| t.movement_type == movement),
        "{movement} written"
    );

    let run = controls::run_reconciliation(&db, RunInventoryReconciliationInput { actor_id: Some(1) })
        .await
        .expect("reconciliation");
    let findings = controls::list_reconciliation_findings(&db, run.id)
        .await
        .expect("findings");
    assert_eq!(run.drift_rows, 0, "{movement} must count in the ledger: {findings:?}");

    // The same movement coming from another PC is applied like the local command did.
    let aux_loc = insert_aux_location(&db, &format!("AUX-{movement}")).await;
    let warehouse_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT warehouse_id FROM stock_locations WHERE id = ?",
            [aux_loc.into()],
        ))
        .await
        .expect("warehouse query")
        .expect("warehouse row")
        .try_get("", "warehouse_id")
        .expect("warehouse id");
    crate::inventory::ledger::apply_received_movement(&db, article_id, warehouse_id, aux_loc, movement, 2.0)
        .await
        .expect("received movement is accepted");
    let (on_hand, reserved) = balance_of(&db, article_id, aux_loc).await.expect("aux balance");
    assert!((on_hand - 2.0 * sign).abs() < 1e-9 && reserved.abs() < 1e-9);
}

#[tokio::test]
async fn a_repairable_cancelled_after_release_reconciles_and_applies_from_sync() {
    assert_repairable_write_off_reconciles(
        &["RELEASED", "CANCELLED"],
        "INV-REP-LEDGER-CXL",
        "REPAIRABLE_CANCEL_RESTORE",
        1.0,
    )
    .await;
}

#[tokio::test]
async fn a_repairable_scrapped_after_return_reconciles_and_applies_from_sync() {
    assert_repairable_write_off_reconciles(
        &["RELEASED", "SENT_FOR_REPAIR", "RETURNED_FROM_REPAIR", "SCRAPPED"],
        "INV-REP-LEDGER-SCRAP",
        "REPAIRABLE_SCRAP_WRITEOFF",
        -1.0,
    )
    .await;
}

/// A goods receipt into a location below zero (left there by movements received from another PC)
/// is accepted (CMMS Advisor decision 1): a partial receipt leaves the item open, the receipt that
/// brings the stock back to zero or more closes it as "régularisé par réception".
#[tokio::test]
async fn a_goods_receipt_into_a_negative_location_is_accepted_and_closes_it_once_back_to_zero() {
    use crate::inventory::negative_stock::{self, CLOSURE_RECEIPT};
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-NEG-GR").await;
    let location_id = main_location_id(&db).await;
    let supplier_id = first_supplier_id(&db).await;
    // What two PCs issuing the same last units leave once both issues are applied here.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at)
         SELECT ?, warehouse_id, id, -5, 0, -5, strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM stock_locations WHERE id = ?",
        [article_id.into(), location_id.into()],
    ))
    .await
    .expect("negative balance");
    assert_eq!(
        negative_stock::list_reviews(&db, false)
            .await
            .expect("list")
            .items
            .len(),
        1
    );

    let req = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id,
            preferred_location_id: Some(location_id),
            requested_qty: 7.0,
            demand_source_type: "REORDER".to_string(),
            demand_source_id: Some(4545),
            demand_source_ref: Some("ROR-4545".to_string()),
            demand_source_line_id: None,
            source_reservation_id: None,
            source_reorder_trigger: Some("threshold_crossed".to_string()),
            purchase_priority: None,
            reason: Some("negative location".to_string()),
            actor_id: None,
        },
    )
    .await
    .expect("create requisition");
    let mut req = req;
    for (status, actor) in [("SUBMITTED", None), ("APPROVED", Some(1))] {
        req = procurement::transition_procurement_requisition(
            &db,
            TransitionProcurementRequisitionInput {
                requisition_id: req.id,
                expected_row_version: req.row_version,
                next_status: status.to_string(),
                reason: None,
                note: None,
                actor_id: actor,
            },
        )
        .await
        .expect("requisition transition");
    }
    let mut po = procurement::create_purchase_order_from_requisition(
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
    for (status, actor) in [("SUBMITTED", None), ("APPROVED", Some(1))] {
        po = procurement::transition_purchase_order(
            &db,
            TransitionPurchaseOrderInput {
                purchase_order_id: po.id,
                expected_row_version: po.row_version,
                next_status: status.to_string(),
                reason: None,
                note: None,
                actor_id: actor,
            },
        )
        .await
        .expect("po transition");
    }
    let po_line = procurement::list_purchase_order_lines(&db, po.id)
        .await
        .expect("po lines")
        .remove(0);
    let receive = |qty: f64| ReceiveGoodsInput {
        purchase_order_id: po.id,
        lines: vec![ReceivePurchaseOrderLineInput {
            po_line_id: po_line.id,
            article_id,
            location_id,
            received_qty: qty,
            accepted_qty: qty,
            rejected_qty: 0.0,
            rejection_reason: None,
            lots: None,
            entered_unit_value_id: None,
        }],
        fulfillment_action: None,
        actor_id: None,
        exchange_rate: None,
        posting_date: None,
    };

    procurement::receive_purchase_order_goods(&db, receive(3.0))
        .await
        .expect("a partial receipt into a location below zero is accepted");
    let (on_hand, _) = balance_of(&db, article_id, location_id).await.expect("balance");
    assert!((on_hand + 2.0).abs() < 1e-9, "−5 + 3, got {on_hand}");
    let list = negative_stock::list_reviews(&db, true).await.expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].status, "open", "still below zero: the item stays open");

    procurement::receive_purchase_order_goods(&db, receive(4.0))
        .await
        .expect("the rest of the order");
    let list = negative_stock::list_reviews(&db, true).await.expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].status, "closed");
    assert_eq!(list.items[0].closure_kind.as_deref(), Some(CLOSURE_RECEIPT));
    assert_eq!(list.items[0].closing_movement_type.as_deref(), Some("GR_ACCEPT"));
    assert!((list.items[0].on_hand_qty - 2.0).abs() < 1e-9);
}

/// A second account of the seeded database, approving count variances in these tests (four-eyes:
/// the counter of the tests is user 1).
async fn count_reviewer(db: &DatabaseConnection) -> i64 {
    let existing = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'countreviewer'".to_string(),
        ))
        .await
        .expect("reviewer");
    if let Some(row) = existing {
        return row.try_get::<i64>("", "id").expect("id");
    }
    i64::from(crate::auth::scope_tests::user(db, "countreviewer").await)
}

/// A plant setting row written as Settings would (tests).
async fn set_tenant_setting(db: &DatabaseConnection, key: &str, value_json: &str) {
    crate::settings::set_setting(db, key, "tenant", value_json, None, "test")
        .await
        .expect("setting");
}

/// A session counting `counted` of an article holding 10 at the main location, entered by `counter`.
async fn counted_line(
    db: &DatabaseConnection,
    code: &str,
    counted: f64,
    counter: Option<i64>,
) -> crate::inventory::domain::InventoryCountLine {
    let article_id = create_test_article(db, code).await;
    let location_id = main_location_id(db).await;
    queries::adjust_stock(
        db,
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
        db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(location_id),
            actor_id: counter,
        },
    )
    .await
    .expect("session");
    controls::upsert_count_line(
        db,
        UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id,
            location_id,
            counted_qty: counted,
            variance_reason_code: Some("COUNT_ERROR".to_string()),
            expected_row_version: None,
            actor_id: counter,
        },
    )
    .await
    .expect("line")
}

fn approval(line: &crate::inventory::domain::InventoryCountLine, reviewer_id: i64) -> ApproveInventoryCountLineInput {
    ApproveInventoryCountLineInput {
        line_id: line.id,
        expected_row_version: line.row_version,
        reviewer_id,
        reviewer_evidence: "Recompté avec le magasinier".to_string(),
    }
}

fn gate_code(err: AppError) -> String {
    match err {
        AppError::ValidationFailed(messages) => messages[0].split(':').next().unwrap_or_default().to_string(),
        other => format!("{other:?}"),
    }
}

/// The only planning site of the organisation, created here, with its four-eyes opt-out.
pub(super) async fn single_site_opting_out(
    db: &DatabaseConnection,
    factory: &mut crate::sync::test_support::RowFactory,
    opt_out: bool,
) {
    let node = factory.create(db, "org_nodes").await.expect("site");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_node_types SET can_own_work = 1 WHERE id = (SELECT node_type_id FROM org_nodes WHERE id = ?)",
        [node.rowid.into()],
    ))
    .await
    .expect("planning site");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes SET deleted_at = CASE WHEN id = ? THEN NULL ELSE strftime('%Y-%m-%dT%H:%M:%SZ','now') END          WHERE id = ? OR node_type_id IN (SELECT id FROM org_node_types WHERE can_own_work = 1)",
        [node.rowid.into(), node.rowid.into()],
    ))
    .await
    .expect("the only live site");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO site_settings (entity_sync_id, site_node_id, setting_key, value_json, updated_at, row_version) \
         VALUES (?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            node.rowid.into(),
            crate::reliability::study::decisions::SELF_APPROVAL_SETTING.into(),
            (if opt_out { "true" } else { "false" }).into(),
        ],
    ))
    .await
    .expect("site rule");
    assert_eq!(
        crate::inventory::count_four_eyes::store_rule(db)
            .await
            .expect("rule")
            .site_id,
        Some(node.rowid),
        "exactly one site"
    );
}

/// D11: above the tolerance the counter does not approve; another person does; within it the
/// counter may; a line whose counter was not recorded is entered again.
#[tokio::test]
async fn a_variance_above_the_tolerance_is_approved_by_someone_other_than_its_counter() {
    let db = setup_db().await;
    set_tenant_setting(&db, "inventory.count_four_eyes_percent_threshold", "25").await;

    let above = counted_line(&db, "INV-4E-ABOVE", 6.0, Some(1)).await;
    assert_eq!((above.is_critical, above.counted_by_id), (1, Some(1)));
    let refused = controls::approve_count_line(&db, approval(&above, 1))
        .await
        .expect_err("the counter approves a variance above the tolerance");
    assert_eq!(
        gate_code(refused),
        crate::inventory::count_four_eyes::REFUSED_SELF_APPROVAL
    );
    let approved = controls::approve_count_line(&db, approval(&above, count_reviewer(&db).await))
        .await
        .expect("a second person approves");
    assert_eq!(
        (approved.approved_by_id, approved.self_approved),
        (Some(count_reviewer(&db).await), 0)
    );
    let flagged = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT details_json FROM audit_events WHERE action_code = 'inventory.count.line_approved' \
             ORDER BY id DESC LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("audit")
        .expect("the approval is audited")
        .try_get::<String>("", "details_json")
        .expect("details");
    assert!(flagged.contains("\"self_approved\":false"));
    // The approval keeps the tolerance: the line's flag judged at entry and the thresholds now.
    let audit: serde_json::Value = serde_json::from_str(&flagged).expect("json");
    assert_eq!(
        audit["four_eyes_tolerance"]["above_tolerance_at_entry"],
        serde_json::json!(true)
    );
    assert_eq!(
        audit["four_eyes_tolerance"]["at_approval"]["percent_threshold"],
        serde_json::json!(25.0)
    );
    assert_eq!(
        audit["four_eyes_tolerance"]["at_approval"]["value_threshold"],
        serde_json::Value::Null,
        "blank stays blank"
    );

    let within = counted_line(&db, "INV-4E-WITHIN", 8.0, Some(1)).await;
    assert_eq!(within.is_critical, 0, "20 % is within 25 %");
    let approved = controls::approve_count_line(&db, approval(&within, 1))
        .await
        .expect("the counter approves a variance within the tolerance");
    // No four-eyes was needed: nothing is stored as a self-approval, and a variance within the
    // tolerance approved by its own counter is no auto-approbation on read (CMMS Advisor Q14).
    assert_eq!(
        approved.self_approved, 0,
        "within the tolerance, approved by its counter: no auto-approbation"
    );
    assert_eq!(
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT self_approved FROM inventory_count_lines WHERE id = ?",
            [approved.id.into()],
        ))
        .await
        .expect("query")
        .expect("line")
        .try_get::<i64>("", "self_approved")
        .expect("stored"),
        0,
        "nothing stored is rewritten"
    );
    let listed = controls::list_count_lines(&db, approved.session_id)
        .await
        .expect("lines")
        .into_iter()
        .find(|l| l.id == approved.id)
        .expect("listed");
    assert_eq!(listed.self_approved, 0, "not listed as an auto-approbation");
    // Above the tolerance and approved by its counter (as another PC's version would hold it, not
    // flagged): shown and listed as an auto-approbation, nothing stored rewritten (Q13, Q14).
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_lines SET approved_by_id = counted_by_id, approved_at = ?, self_approved = 0
         WHERE id = ?",
        ["2026-10-25T09:00:00Z".into(), above.id.into()],
    ))
    .await
    .expect("approved by its counter");
    let listed = controls::list_count_lines(&db, above.session_id)
        .await
        .expect("lines")
        .into_iter()
        .find(|l| l.id == above.id)
        .expect("listed");
    assert_eq!(
        (listed.is_critical, listed.self_approved),
        (1, 1),
        "above the tolerance, approved by its counter: listed as an auto-approbation"
    );
    let short = controls::approve_count_line(
        &db,
        ApproveInventoryCountLineInput {
            reviewer_evidence: "  court ".to_string(),
            ..approval(
                &controls::list_count_lines(&db, within.session_id).await.expect("l")[0],
                1,
            )
        },
    )
    .await
    .expect_err("the 8-character note is still required");
    assert_eq!(gate_code(short), "GATE_INV_COUNT_EVIDENCE");

    let unknown = counted_line(&db, "INV-4E-NOBODY", 6.0, None).await;
    let refused = controls::approve_count_line(&db, approval(&unknown, count_reviewer(&db).await))
        .await
        .expect_err("the counter of a variance above the tolerance was not recorded");
    assert_eq!(
        gate_code(refused),
        crate::inventory::count_four_eyes::REFUSED_COUNTER_UNKNOWN
    );
}

/// D11: under the four-eyes opt-out of the organisation's only site, the counter may approve a
/// variance above the tolerance; it is flagged "auto-approbation". Without the opt-out, or with
/// several sites, four-eyes applies.
#[tokio::test]
async fn the_only_sites_opt_out_lets_the_counter_approve_flagged_as_a_self_approval() {
    let db = setup_db().await;
    let mut factory = crate::sync::test_support::RowFactory::new();
    single_site_opting_out(&db, &mut factory, false).await;
    let line = counted_line(&db, "INV-4E-SITE-OFF", 6.0, Some(1)).await;
    assert_eq!(
        gate_code(
            controls::approve_count_line(&db, approval(&line, 1))
                .await
                .expect_err("four-eyes")
        ),
        crate::inventory::count_four_eyes::REFUSED_SELF_APPROVAL
    );

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE site_settings SET value_json = 'true'".to_string(),
    ))
    .await
    .expect("opt out");
    let approved = controls::approve_count_line(&db, approval(&line, 1))
        .await
        .expect("self-approval under the opt-out");
    assert_eq!((approved.approved_by_id, approved.self_approved), (Some(1), 1));

    // A second site: the stores' rule can no longer come from one site.
    let second = factory.create(&db, "org_nodes").await.expect("second site");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes SET deleted_at = NULL,
                node_type_id = (SELECT node_type_id FROM org_nodes WHERE id IN (SELECT site_node_id FROM site_settings))
         WHERE id = ?",
        [second.rowid.into()],
    ))
    .await
    .expect("second planning site");
    let other = counted_line(&db, "INV-4E-TWO-SITES", 6.0, Some(1)).await;
    assert_eq!(
        gate_code(
            controls::approve_count_line(&db, approval(&other, 1))
                .await
                .expect_err("two sites")
        ),
        crate::inventory::count_four_eyes::REFUSED_SELF_APPROVAL
    );
}

/// D11: an optional threshold set blank has no row (blank is never zero) and its change is kept
/// in the settings history; the retired critical variance is not in the catalog.
#[tokio::test]
async fn a_four_eyes_threshold_set_blank_has_no_row() {
    let db = setup_db().await;
    let key = crate::inventory::count_four_eyes::SETTING_VALUE_THRESHOLD;
    assert_eq!(
        crate::settings::catalog::optional_decimal_setting(&db, key)
            .await
            .expect("read"),
        None,
        "not seeded"
    );
    set_tenant_setting(&db, key, "0").await;
    assert_eq!(
        crate::settings::catalog::optional_decimal_setting(&db, key)
            .await
            .expect("read"),
        Some(0.0),
        "zero is a value"
    );
    set_tenant_setting(&db, key, "null").await;
    assert_eq!(
        crate::settings::catalog::optional_decimal_setting(&db, key)
            .await
            .expect("read"),
        None
    );
    let history = crate::settings::list_change_events(&db, 10, Some(key))
        .await
        .expect("history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].new_value_json, None, "cleared");
    assert!(crate::settings::catalog::definition("inventory.count_critical_variance").is_none());
}

/// AU-68: a count line is entered again only against the version its author saw; a stale version,
/// or none on a line already entered, is refused and the line is left as it was.
#[tokio::test]
async fn a_count_line_is_entered_again_only_against_its_current_version() {
    let db = setup_db().await;
    let article_id = create_test_article(&db, "INV-CC-AU68").await;
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
    let entry = |counted_qty: f64, expected_row_version: Option<i64>| UpsertInventoryCountLineInput {
        session_id: session.id,
        article_id,
        location_id,
        counted_qty,
        variance_reason_code: Some("COUNT_ERROR".to_string()),
        expected_row_version,
        actor_id: Some(1),
    };
    let changed_elsewhere = |err: AppError| match err {
        AppError::ValidationFailed(messages) => messages[0].starts_with("GATE_RECORD_CHANGED_ELSEWHERE"),
        _ => false,
    };

    let refused_first = controls::upsert_count_line(&db, entry(9.0, Some(1)))
        .await
        .expect_err("a version given for a line not entered yet");
    assert!(changed_elsewhere(refused_first));

    let first = controls::upsert_count_line(&db, entry(9.0, None))
        .await
        .expect("first entry");
    assert!((first.counted_qty - 9.0).abs() < 1e-9);

    let without_version = controls::upsert_count_line(&db, entry(8.0, None))
        .await
        .expect_err("no version on a line already entered");
    assert!(changed_elsewhere(without_version));

    let again = controls::upsert_count_line(&db, entry(8.0, Some(first.row_version)))
        .await
        .expect("entered again against its version");
    assert!((again.counted_qty - 8.0).abs() < 1e-9);
    assert!(again.row_version > first.row_version);

    let stale = controls::upsert_count_line(&db, entry(7.0, Some(first.row_version)))
        .await
        .expect_err("a stale version");
    assert!(changed_elsewhere(stale));
    let lines = controls::list_count_lines(&db, session.id).await.expect("lines");
    assert_eq!(lines.len(), 1);
    assert!(
        (lines[0].counted_qty - 8.0).abs() < 1e-9,
        "the refused entries changed nothing"
    );
    assert_eq!(lines[0].row_version, again.row_version);
}

/// D11 review (blocking 1): a variance whose counter was not recorded (entered before migration
/// 259) is never approved without four-eyes, even when its flag says within the tolerance (the
/// flag of the retired rule): it is entered again.
#[tokio::test]
async fn a_variance_without_a_recorded_counter_is_entered_again_whatever_its_flag() {
    let db = setup_db().await;
    set_tenant_setting(&db, "inventory.count_four_eyes_percent_threshold", "25").await;
    let within = counted_line(&db, "INV-4E-OLD-WITHIN", 8.0, None).await;
    assert_eq!((within.is_critical, within.counted_by_id), (0, None));
    let refused = controls::approve_count_line(&db, approval(&within, count_reviewer(&db).await))
        .await
        .expect_err("no counter recorded");
    assert_eq!(
        gate_code(refused),
        crate::inventory::count_four_eyes::REFUSED_COUNTER_UNKNOWN
    );
}

/// D11 review: a line is approved once, while its session is still counted or reviewed: approving
/// it again, or approving a line of a cancelled count, is refused (its approver is never rewritten).
#[tokio::test]
async fn a_count_line_is_approved_once_and_only_while_its_session_is_open() {
    let db = setup_db().await;
    let reviewer = count_reviewer(&db).await;
    let line = counted_line(&db, "INV-4E-ONCE", 6.0, Some(1)).await;
    let approved = controls::approve_count_line(&db, approval(&line, reviewer))
        .await
        .expect("approved");
    assert_eq!(
        gate_code(
            controls::approve_count_line(&db, approval(&approved, reviewer))
                .await
                .expect_err("twice")
        ),
        controls::REFUSED_COUNT_LINE_ALREADY_APPROVED
    );

    let other = counted_line(&db, "INV-4E-CANCELLED", 6.0, Some(1)).await;
    let session = controls::list_count_sessions(&db)
        .await
        .expect("sessions")
        .into_iter()
        .find(|s| s.id == other.session_id)
        .expect("session");
    controls::transition_count_session(
        &db,
        TransitionInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            next_status: "cancelled".to_string(),
            reason: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("cancelled");
    assert_eq!(
        gate_code(
            controls::approve_count_line(&db, approval(&other, reviewer))
                .await
                .expect_err("cancelled count")
        ),
        controls::REFUSED_COUNT_LINE_APPROVAL_CLOSED
    );
}

/// D11 review: a line entered again by another person records that person as its counter and loses
/// the approval given to the previous entry, its self-approval flag included.
#[tokio::test]
async fn a_line_entered_again_by_another_person_changes_its_counter_and_drops_the_self_approval() {
    let db = setup_db().await;
    let mut factory = crate::sync::test_support::RowFactory::new();
    single_site_opting_out(&db, &mut factory, true).await;
    let line = counted_line(&db, "INV-4E-REENTRY", 6.0, Some(1)).await;
    let approved = controls::approve_count_line(&db, approval(&line, 1))
        .await
        .expect("self-approval under the opt-out");
    assert_eq!(approved.self_approved, 1);
    let reviewer = count_reviewer(&db).await;
    let again = controls::upsert_count_line(
        &db,
        UpsertInventoryCountLineInput {
            session_id: line.session_id,
            article_id: line.article_id,
            location_id: line.location_id,
            counted_qty: 6.0,
            variance_reason_code: Some("COUNT_ERROR".to_string()),
            expected_row_version: Some(approved.row_version),
            actor_id: Some(reviewer),
        },
    )
    .await
    .expect("entered again");
    assert_eq!(again.counted_by_id, Some(reviewer));
    assert_eq!((again.self_approved, again.approved_by_id), (0, None));
}

/// The session of `line`, moved through counting, submitted and approved, then posted by user 1.
async fn post_line_session(
    db: &DatabaseConnection,
    line: &crate::inventory::domain::InventoryCountLine,
) -> crate::inventory::domain::InventoryCountLine {
    let session_now = || async {
        controls::list_count_sessions(db)
            .await
            .expect("sessions")
            .into_iter()
            .find(|s| s.id == line.session_id)
            .expect("session")
    };
    for next in ["counting", "submitted", "approved"] {
        let session = session_now().await;
        controls::transition_count_session(
            db,
            TransitionInventoryCountSessionInput {
                session_id: session.id,
                expected_row_version: session.row_version,
                next_status: next.to_string(),
                reason: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect(next);
    }
    let session = session_now().await;
    controls::post_count_session(
        db,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(1),
        },
    )
    .await
    .expect("posted");
    controls::list_count_lines(db, line.session_id).await.expect("lines")[0].clone()
}

/// Reverses the posted session of `line` (user 1).
async fn reverse_line_session(
    db: &DatabaseConnection,
    line: &crate::inventory::domain::InventoryCountLine,
) -> crate::inventory::domain::InventoryCountLine {
    let session = controls::list_count_sessions(db)
        .await
        .expect("sessions")
        .into_iter()
        .find(|s| s.id == line.session_id)
        .expect("session");
    controls::reverse_count_session(
        db,
        ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Feuille de comptage erronée".to_string(),
            actor_id: Some(1),
        },
    )
    .await
    .expect("reversed");
    controls::list_count_lines(db, line.session_id).await.expect("lines")[0].clone()
}

/// CMMS Advisor Q8 (review of a9c38354): a second approval needs the count-approval right
/// (`inv.manage`) and a note of at least 8 characters, and only a posted, unreversed line whose
/// variance above the tolerance its counter approved needs one — not before posting, not after the
/// count's reversal, not within the tolerance, not when someone else approved it.
#[tokio::test]
async fn a_second_approval_needs_the_right_a_note_and_a_posted_self_approval() {
    let db = setup_db().await;
    let mut factory = crate::sync::test_support::RowFactory::new();
    single_site_opting_out(&db, &mut factory, true).await;
    let manager = i64::from(
        crate::inventory::negative_stock_tests::user_with(
            &db,
            "secondapprovalmanager",
            &[crate::rbac::permissions::INV_MANAGE],
        )
        .await,
    );
    let viewer = i64::from(
        crate::inventory::negative_stock_tests::user_with(
            &db,
            "secondapprovalviewer",
            &[crate::rbac::permissions::INV_VIEW],
        )
        .await,
    );
    let second = |line: crate::inventory::domain::InventoryCountLine, by: i64, note: &'static str| {
        let db = &db;
        async move {
            controls::second_approve_count_line(
                db,
                crate::inventory::domain::SecondApproveInventoryCountLineInput {
                    line_id: line.id,
                    expected_row_version: line.row_version,
                    reviewer_id: by,
                    reviewer_evidence: note.to_string(),
                },
            )
            .await
        }
    };
    let note = "Comptage vérifié par un second";
    let not_needed = controls::REFUSED_SECOND_APPROVAL_NOT_NEEDED;

    // Self-approved by its counter (under the opt-out), not posted yet.
    let line = counted_line(&db, "INV-2A-SELF", 6.0, Some(1)).await;
    let approved = controls::approve_count_line(&db, approval(&line, 1))
        .await
        .expect("self-approval under the opt-out");
    assert_eq!(approved.self_approved, 1);
    assert_eq!(
        gate_code(second(approved.clone(), manager, note).await.expect_err("not posted")),
        not_needed
    );
    // Posted: the right, then the note.
    let posted = post_line_session(&db, &approved).await;
    let denied = second(posted.clone(), viewer, note).await.expect_err("inv.view only");
    assert!(matches!(denied, AppError::PermissionDenied(_)), "{denied:?}");
    for short in ["", "court", "  abcdefg  "] {
        assert_eq!(
            gate_code(second(posted.clone(), manager, short).await.expect_err(short)),
            "GATE_INV_COUNT_EVIDENCE",
            "{short:?}"
        );
    }
    // Reversed: nothing it moved is kept, nothing to approve a second time.
    let reversed = reverse_line_session(&db, &posted).await;
    assert_eq!(
        gate_code(second(reversed, manager, note).await.expect_err("reversed")),
        not_needed
    );

    // Approved by someone other than its counter.
    let other = counted_line(&db, "INV-2A-OTHER", 6.0, Some(1)).await;
    let other = controls::approve_count_line(&db, approval(&other, count_reviewer(&db).await))
        .await
        .expect("a second person approves");
    let other = post_line_session(&db, &other).await;
    assert_eq!(
        gate_code(
            second(other, manager, note)
                .await
                .expect_err("approved by another person")
        ),
        not_needed
    );

    // Within the tolerance: the counter's approval is no self-approval.
    set_tenant_setting(&db, "inventory.count_four_eyes_percent_threshold", "50").await;
    let within = counted_line(&db, "INV-2A-WITHIN", 8.0, Some(1)).await;
    assert_eq!(within.is_critical, 0);
    let within = controls::approve_count_line(&db, approval(&within, 1))
        .await
        .expect("the counter approves within the tolerance");
    let within = post_line_session(&db, &within).await;
    assert_eq!(
        gate_code(second(within, manager, note).await.expect_err("within the tolerance")),
        not_needed
    );
}

/// Review of 9ebd482c: the approval guard holds on a posted count and on a reversed one, not only
/// on a cancelled one: the approver of the line is never rewritten.
#[tokio::test]
async fn a_line_of_a_posted_or_reversed_count_is_never_approved_again() {
    let db = setup_db().await;
    let reviewer = count_reviewer(&db).await;
    let line = counted_line(&db, "INV-4E-POSTED", 6.0, Some(1)).await;
    let approved = controls::approve_count_line(&db, approval(&line, reviewer))
        .await
        .expect("approved");
    let posted = post_line_session(&db, &approved).await;
    let refused_on = |line: crate::inventory::domain::InventoryCountLine, status: &'static str| {
        let db = &db;
        async move {
            let refused = controls::approve_count_line(db, approval(&line, 1))
                .await
                .expect_err(status);
            let message = format!("{refused:?}");
            assert_eq!(
                gate_code(refused),
                controls::REFUSED_COUNT_LINE_APPROVAL_CLOSED,
                "{status}"
            );
            assert!(message.contains(&format!("status={status}")), "{message}");
            let kept = &controls::list_count_lines(db, line.session_id).await.expect("lines")[0];
            assert_eq!(kept.approved_by_id, Some(reviewer), "{status}: the approver is kept");
        }
    };
    refused_on(posted.clone(), "posted").await;
    let reversed = reverse_line_session(&db, &posted).await;
    refused_on(reversed, "reversed").await;
}

/// CMMS Advisor Q10: a count line keeps the unit cost it was judged with at entry and the
/// variance's value at that cost (none without a unit cost), for the PCs that receive it.
#[tokio::test]
async fn a_count_line_keeps_the_unit_cost_and_value_it_was_entered_with() {
    let db = setup_db().await;
    set_tenant_setting(&db, crate::inventory::count_four_eyes::SETTING_VALUE_THRESHOLD, "9").await;
    let without = counted_line(&db, "INV-4E-NOCOST", 6.0, Some(1)).await;
    let article_id = create_test_article(&db, "INV-4E-COST").await;
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
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_balances SET moving_avg_unit_cost = 2.5 WHERE article_id = ? AND location_id = ?",
        [article_id.into(), location_id.into()],
    ))
    .await
    .expect("cost");
    let session = controls::create_count_session(
        &db,
        CreateInventoryCountSessionInput {
            warehouse_id: 1,
            location_id: Some(location_id),
            actor_id: Some(1),
        },
    )
    .await
    .expect("session");
    let with = controls::upsert_count_line(
        &db,
        UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id,
            location_id,
            counted_qty: 6.0,
            variance_reason_code: Some("COUNT_ERROR".to_string()),
            expected_row_version: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("line");
    assert_eq!(with.is_critical, 1, "4 × 2.5 = 10 is above 9");
    let entry = |id: i64| {
        let db = &db;
        async move {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT entry_unit_cost, entry_variance_value FROM inventory_count_lines WHERE id = ?",
                    [id.into()],
                ))
                .await
                .expect("query")
                .expect("line");
            (
                row.try_get::<Option<f64>>("", "entry_unit_cost").expect("cost"),
                row.try_get::<Option<f64>>("", "entry_variance_value").expect("value"),
            )
        }
    };
    assert_eq!(entry(with.id).await, (Some(2.5), Some(-10.0)));
    assert_eq!(entry(without.id).await, (None, None), "no unit cost: none kept");
}
