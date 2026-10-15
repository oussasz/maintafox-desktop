//! Tests of the inventory benchmark features (migration 184): issue desk, lots / serials / FEFO,
//! valuation periods, ABC by consumption value, supplier currency at receipt, approval limits and
//! separation of duties, multi-line requisitions and partial receipts.

use chrono::{Datelike, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    CloseValuationPeriodInput, CreateMultiLineRequisitionInput, CreateProcurementRequisitionInput,
    CreatePurchaseOrderFromRequisitionInput, CurrencyExchangeRateInput, InventoryArticleInput, InventoryIssueInput,
    InventoryReserveInput, InventoryStockAdjustInput, InventoryStockFilter, IssueStockRequest, NewLotInput,
    ProcurementApprovalLimitInput, ProcurementRequisition, PurchaseOrder, ReceiveGoodsInput,
    ReceivePurchaseOrderLineInput, RequisitionLineInput, TransitionProcurementRequisitionInput,
    TransitionPurchaseOrderInput,
};
use crate::inventory::queries::AdjustStockOptions;
use crate::inventory::{approvals, fx, issue, lots, periods, procurement, queries};

// ── Fixtures ───────────────────────────────────────────────────────────────────────────────

async fn setup_db() -> DatabaseConnection {
    crate::db::test_db::seeded().await
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("value")
}

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("exec");
}

async fn lookup_id(db: &DatabaseConnection, domain_key: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id
         WHERE ld.domain_key = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL
         ORDER BY lv.sort_order ASC, lv.id ASC LIMIT 1",
        vec![domain_key.into()],
    )
    .await
}

async fn main_location_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1",
        vec![],
    )
    .await
}

#[derive(Default, Clone, Copy)]
struct Tracking {
    batch: bool,
    serial: bool,
    expiry: bool,
}

async fn article(db: &DatabaseConnection, code: &str, tracking: Tracking, barcode: Option<&str>) -> i64 {
    let input = InventoryArticleInput {
        article_code: code.to_string(),
        article_name: format!("Article {code}"),
        family_id: None,
        unit_value_id: lookup_id(db, "inventory.unit_of_measure").await,
        criticality_value_id: None,
        stocking_type_value_id: lookup_id(db, "inventory.stocking_type").await,
        tax_category_value_id: lookup_id(db, "inventory.tax_category").await,
        procurement_category_value_id: None,
        preferred_warehouse_id: None,
        preferred_location_id: None,
        min_stock: 0.0,
        max_stock: None,
        reorder_point: 0.0,
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
        requires_expiration: Some(tracking.expiry),
        shelf_life_days: None,
        requires_batch_tracking: Some(tracking.batch),
        is_active: Some(true),
        barcode: barcode.map(str::to_string),
        requires_serial_tracking: Some(tracking.serial),
    };
    queries::create_article(db, input).await.expect("create article").id
}

async fn work_order(db: &DatabaseConnection, code: &str, status_code: &str) -> i64 {
    exec(
        db,
        "INSERT INTO work_orders (code, type_id, status_id, title, row_version, created_at, updated_at)
         VALUES (?, (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'),
                 (SELECT id FROM work_order_statuses WHERE code = ?), 'Pump repair', 1,
                 datetime('now'), datetime('now'))",
        vec![code.into(), status_code.into()],
    )
    .await;
    scalar_i64(db, "SELECT id FROM work_orders WHERE code = ?", vec![code.into()]).await
}

async fn user(db: &DatabaseConnection, username: &str) -> i64 {
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin,
             force_password_change, created_at, updated_at)
         VALUES (?, ?, ?, 'local', 1, 0, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![format!("inv-user-{username}").into(), username.into(), username.into()],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await
}

async fn role_for(db: &DatabaseConnection, role_name: &str, user_id: i64) -> i64 {
    exec(
        db,
        "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at)
         VALUES (?, ?, 'Buyer', 0, 'custom', 'active', strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![format!("inv-role-{role_name}").into(), role_name.into()],
    )
    .await;
    let role_id = scalar_i64(db, "SELECT id FROM roles WHERE name = ?", vec![role_name.into()]).await;
    exec(
        db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
             valid_from, valid_to, assigned_by_id, notes, created_at, updated_at)
         VALUES (?, ?, ?, 'tenant', NULL, NULL, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![format!("inv-usa-{role_name}").into(), user_id.into(), role_id.into()],
    )
    .await;
    role_id
}

fn entry(article_id: i64, location_id: i64, qty: f64) -> InventoryStockAdjustInput {
    InventoryStockAdjustInput {
        article_id,
        location_id,
        delta_qty: qty,
        reason_code: Some("ENTREE_ACHAT".to_string()),
        notes: None,
        source_ref: None,
    }
}

async fn stock_in(db: &DatabaseConnection, article_id: i64, location_id: i64, qty: f64, cost: f64) {
    queries::adjust_stock_costed(db, entry(article_id, location_id, qty), Some(cost))
        .await
        .expect("stock entry");
}

async fn stock_in_lot(
    db: &DatabaseConnection,
    article_id: i64,
    location_id: i64,
    qty: f64,
    lot: NewLotInput,
) -> AppResult<()> {
    queries::adjust_stock_full(
        db,
        entry(article_id, location_id, qty),
        AdjustStockOptions {
            unit_cost: Some(2.0),
            new_lot: Some(lot),
            ..Default::default()
        },
    )
    .await
    .map(|_| ())
}

fn issue_request(article_id: i64, location_id: i64, qty: f64, work_order_id: i64) -> IssueStockRequest {
    IssueStockRequest {
        article_id,
        location_id,
        quantity: qty,
        work_order_id: Some(work_order_id),
        reservation_id: None,
        lot_id: None,
        notes: None,
        actor_id: None,
    }
}

fn message(err: &AppError) -> String {
    format!("{err:?}")
}

async fn on_hand_and_reserved(db: &DatabaseConnection, article_id: i64, location_id: i64) -> (f64, f64, Option<f64>) {
    let balances = queries::list_stock_balances(
        db,
        InventoryStockFilter {
            article_id: Some(article_id),
            warehouse_id: None,
            low_stock_only: None,
        },
    )
    .await
    .expect("balances");
    let b = balances
        .iter()
        .find(|b| b.location_id == location_id)
        .expect("balance at location");
    (b.on_hand_qty, b.reserved_qty, b.moving_avg_unit_cost)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

async fn transition_req(
    db: &DatabaseConnection,
    req: &ProcurementRequisition,
    next: &str,
    actor: Option<i64>,
) -> AppResult<ProcurementRequisition> {
    procurement::transition_procurement_requisition(
        db,
        TransitionProcurementRequisitionInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            next_status: next.to_string(),
            reason: None,
            note: None,
            actor_id: actor,
        },
    )
    .await
}

async fn transition_po(
    db: &DatabaseConnection,
    po: &PurchaseOrder,
    next: &str,
    actor: Option<i64>,
) -> AppResult<PurchaseOrder> {
    procurement::transition_purchase_order(
        db,
        TransitionPurchaseOrderInput {
            purchase_order_id: po.id,
            expected_row_version: po.row_version,
            next_status: next.to_string(),
            reason: None,
            note: None,
            actor_id: actor,
        },
    )
    .await
}

fn receipt_line(po_line_id: i64, article_id: i64, location_id: i64, qty: f64) -> ReceivePurchaseOrderLineInput {
    ReceivePurchaseOrderLineInput {
        po_line_id,
        article_id,
        location_id,
        received_qty: qty,
        accepted_qty: qty,
        rejected_qty: 0.0,
        rejection_reason: None,
        lots: None,
    }
}

// ── Issue desk ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn issue_desk_reserves_and_issues_to_an_open_work_order_at_average_cost() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let article_id = article(&db, "INV-DESK-001", Tracking::default(), None).await;
    stock_in(&db, article_id, location_id, 10.0, 5.0).await;
    let wo_id = work_order(&db, "OT-DESK-0001", "in_progress").await;

    let outcome = issue::issue_stock(&db, issue_request(article_id, location_id, 3.0, wo_id))
        .await
        .expect("issue to OT");
    assert_eq!(outcome.work_order_code.as_deref(), Some("OT-DESK-0001"));
    assert_eq!(outcome.lines.len(), 1);
    assert!(close(outcome.lines[0].unit_cost.expect("cost"), 5.0));
    assert!(close(outcome.total_cost.expect("total"), 15.0));
    let (on_hand, reserved, _) = on_hand_and_reserved(&db, article_id, location_id).await;
    assert!(
        close(on_hand, 7.0) && close(reserved, 0.0),
        "direct issue = reserve + issue"
    );

    let source_ref: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT source_ref FROM inventory_transactions WHERE id = ?",
            [outcome.lines[0].transaction_id.into()],
        ))
        .await
        .expect("query")
        .expect("issue row")
        .try_get("", "source_ref")
        .expect("source_ref");
    assert_eq!(source_ref, "OT-DESK-0001");

    // A work order that is not open for parts is refused.
    let closed_status: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT code FROM work_order_statuses
             WHERE code NOT IN ('planning', 'ready', 'in_progress', 'on_hold') ORDER BY id LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("query")
        .expect("a non-open status")
        .try_get("", "code")
        .expect("code");
    let closed_wo = work_order(&db, "OT-DESK-0002", &closed_status).await;
    assert!(
        issue::issue_stock(&db, issue_request(article_id, location_id, 1.0, closed_wo))
            .await
            .is_err()
    );
    // An issue without a work order or reservation is refused.
    let mut orphan = issue_request(article_id, location_id, 1.0, wo_id);
    orphan.work_order_id = None;
    assert!(issue::issue_stock(&db, orphan).await.is_err());
}

#[tokio::test]
async fn work_order_reservation_issue_posts_the_same_costed_issue() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let article_id = article(&db, "INV-DESK-002", Tracking::default(), None).await;
    stock_in(&db, article_id, location_id, 4.0, 8.0).await;
    let wo_id = work_order(&db, "OT-DESK-0003", "in_progress").await;
    let reservation = queries::reserve_stock(
        &db,
        InventoryReserveInput {
            article_id,
            location_id,
            quantity: 2.0,
            source_type: "WORK_ORDER".to_string(),
            source_id: Some(wo_id),
            source_ref: Some("OT-DESK-0003".to_string()),
            notes: None,
        },
    )
    .await
    .expect("reserve");
    queries::issue_reserved_stock(
        &db,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity: 2.0,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("issue reserved");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_cost, extended_cost FROM inventory_transactions
             WHERE article_id = ? AND movement_type = 'ISSUE'",
            [article_id.into()],
        ))
        .await
        .expect("query")
        .expect("issue row");
    let extended: f64 = row.try_get("", "extended_cost").expect("extended_cost");
    assert!(close(extended, 16.0), "WO-path issues are valued like desk issues");
}

#[tokio::test]
async fn scan_resolves_barcode_article_code_and_serial() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let plain = article(&db, "INV-SCAN-001", Tracking::default(), Some("3760000000017")).await;
    let serial = article(
        &db,
        "INV-SCAN-002",
        Tracking {
            serial: true,
            ..Default::default()
        },
        None,
    )
    .await;
    stock_in_lot(
        &db,
        serial,
        location_id,
        1.0,
        NewLotInput {
            serial_number: Some("SN-0001".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("serial entry");

    let by_barcode = issue::resolve_scan(&db, " 3760000000017 ".to_string())
        .await
        .expect("barcode");
    assert_eq!(
        (by_barcode.article_id, by_barcode.matched_on.as_str()),
        (plain, "barcode")
    );
    let by_code = issue::resolve_scan(&db, "inv-scan-001".to_string())
        .await
        .expect("code");
    assert_eq!(
        (by_code.article_id, by_code.matched_on.as_str()),
        (plain, "article_code")
    );
    let by_serial = issue::resolve_scan(&db, "sn-0001".to_string()).await.expect("serial");
    assert_eq!(by_serial.article_id, serial);
    assert!(by_serial.lot_id.is_some());
    assert!(issue::resolve_scan(&db, "NOTHING-HERE".to_string()).await.is_err());
}

// ── Lots, serials, FEFO, expiry ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn lots_are_captured_at_entry_and_issued_first_expired_first_out() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let tracked = Tracking {
        batch: true,
        expiry: true,
        ..Default::default()
    };
    let article_id = article(&db, "INV-LOT-001", tracked, None).await;
    let today = Utc::now().date_naive();
    let near = (today + Duration::days(10)).to_string();
    let far = (today + Duration::days(60)).to_string();

    // An entry of a tracked article must name its lot; a shelf-life article needs an expiry.
    assert!(queries::adjust_stock(&db, entry(article_id, location_id, 1.0))
        .await
        .is_err());
    assert!(stock_in_lot(
        &db,
        article_id,
        location_id,
        1.0,
        NewLotInput {
            lot_number: Some("L-NODATE".to_string()),
            ..Default::default()
        },
    )
    .await
    .is_err());

    for (lot, expiry) in [("L-FAR", &far), ("L-NEAR", &near)] {
        stock_in_lot(
            &db,
            article_id,
            location_id,
            4.0,
            NewLotInput {
                lot_number: Some(lot.to_string()),
                expiry_date: Some(expiry.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("lot entry");
    }

    let expiring = lots::list_expiring_lots(&db, 30).await.expect("expiring");
    let expiring_lots: Vec<_> = expiring
        .iter()
        .filter(|l| l.article_id == article_id)
        .filter_map(|l| l.lot_number.clone())
        .collect();
    assert_eq!(expiring_lots, vec!["L-NEAR".to_string()]);

    let wo_id = work_order(&db, "OT-LOT-0001", "in_progress").await;
    let outcome = issue::issue_stock(&db, issue_request(article_id, location_id, 5.0, wo_id))
        .await
        .expect("FEFO issue");
    let picked: Vec<(String, f64)> = outcome
        .lines
        .iter()
        .map(|l| (l.lot_number.clone().unwrap_or_default(), l.quantity))
        .collect();
    assert_eq!(picked, vec![("L-NEAR".to_string(), 4.0), ("L-FAR".to_string(), 1.0)]);

    let remaining = lots::list_lot_stock(&db, article_id, Some(location_id))
        .await
        .expect("lot stock");
    let far_left = remaining
        .iter()
        .find(|l| l.lot_number.as_deref() == Some("L-FAR"))
        .expect("far lot");
    assert!(close(far_left.quantity, 3.0));
    assert!(remaining
        .iter()
        .all(|l| l.lot_number.as_deref() != Some("L-NEAR") || l.quantity.abs() < 1e-9));

    // Asking for more than the lots hold is refused.
    assert!(
        issue::issue_stock(&db, issue_request(article_id, location_id, 4.0, wo_id))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn serial_numbers_are_one_unit_each_and_unique_in_stock() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let article_id = article(
        &db,
        "INV-SER-001",
        Tracking {
            serial: true,
            ..Default::default()
        },
        None,
    )
    .await;
    let serial = |sn: &str| NewLotInput {
        serial_number: Some(sn.to_string()),
        ..Default::default()
    };
    stock_in_lot(&db, article_id, location_id, 1.0, serial("SN-A"))
        .await
        .expect("SN-A");
    assert!(
        stock_in_lot(&db, article_id, location_id, 1.0, serial("SN-A"))
            .await
            .is_err(),
        "a serial already in stock cannot be received again"
    );
    assert!(
        stock_in_lot(&db, article_id, location_id, 2.0, serial("SN-B"))
            .await
            .is_err(),
        "a serial is one unit"
    );
    stock_in_lot(&db, article_id, location_id, 1.0, serial("SN-B"))
        .await
        .expect("SN-B");

    let stock = lots::list_lot_stock(&db, article_id, Some(location_id))
        .await
        .expect("stock");
    let sn_b = stock
        .iter()
        .find(|l| l.serial_number.as_deref() == Some("SN-B"))
        .expect("SN-B lot")
        .lot_id;
    let wo_id = work_order(&db, "OT-SER-0001", "in_progress").await;
    let mut request = issue_request(article_id, location_id, 1.0, wo_id);
    request.lot_id = Some(sn_b);
    let outcome = issue::issue_stock(&db, request).await.expect("issue chosen serial");
    assert_eq!(outcome.lines[0].serial_number.as_deref(), Some("SN-B"));
    // Fractions of a serialised item are refused.
    assert!(
        issue::issue_stock(&db, issue_request(article_id, location_id, 0.5, wo_id))
            .await
            .is_err()
    );
}

// ── Valuation periods ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn closing_a_period_freezes_its_valuation_and_refuses_back_dated_postings() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let article_id = article(&db, "INV-PER-001", Tracking::default(), None).await;
    let today = Utc::now().date_naive();
    let this_month = today.with_day(1).expect("first of month");
    let period_end = this_month - Duration::days(1);
    let period_start = period_end.with_day(1).expect("first of previous month");
    let in_period = period_start.to_string();

    // Back-dated entry inside the period: 10 @ 2.
    queries::adjust_stock_full(
        &db,
        entry(article_id, location_id, 10.0),
        AdjustStockOptions {
            unit_cost: Some(2.0),
            posting_date: Some(in_period.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("back-dated entry");
    // A future posting date is refused.
    assert!(queries::adjust_stock_full(
        &db,
        entry(article_id, location_id, 1.0),
        AdjustStockOptions {
            posting_date: Some((today + Duration::days(3)).to_string()),
            ..Default::default()
        },
    )
    .await
    .is_err());

    let period = periods::close_period(
        &db,
        CloseValuationPeriodInput {
            period_start: period_start.to_string(),
            period_end: period_end.to_string(),
            notes: Some("month end".to_string()),
            actor_id: None,
        },
    )
    .await
    .expect("close period");
    assert_eq!(
        period.period_code,
        format!("{:04}-{:02}", period_end.year(), period_end.month())
    );
    assert!(close(period.total_value, 20.0));

    // Postings dated in the closed period are refused with the period named.
    let refused = queries::adjust_stock_full(
        &db,
        entry(article_id, location_id, 1.0),
        AdjustStockOptions {
            unit_cost: Some(9.0),
            posting_date: Some(in_period.clone()),
            ..Default::default()
        },
    )
    .await
    .expect_err("closed period");
    assert!(message(&refused).contains("GATE_INV_PERIOD_CLOSED"), "{refused:?}");
    // Overlapping closes are refused.
    assert!(periods::close_period(
        &db,
        CloseValuationPeriodInput {
            period_start: period_start.to_string(),
            period_end: period_end.to_string(),
            notes: None,
            actor_id: None,
        },
    )
    .await
    .is_err());

    // Today's movement: 5 @ 4 → average (20 + 20) / 15.
    stock_in(&db, article_id, location_id, 5.0, 4.0).await;

    let frozen = periods::valuation_report(&db, Some(period_end.to_string()), "article".to_string())
        .await
        .expect("frozen report");
    assert_eq!(frozen.frozen_period_code.as_deref(), Some(period.period_code.as_str()));
    let frozen_row = frozen
        .rows
        .iter()
        .find(|r| r.group_id == Some(article_id))
        .expect("article row");
    assert!(close(frozen_row.value, 20.0) && close(frozen_row.quantity, 10.0));

    let current = periods::valuation_report(&db, None, "warehouse".to_string())
        .await
        .expect("current report");
    assert!(current.frozen_period_code.is_none());
    assert!(close(current.total_value, 40.0), "{}", current.total_value);
    assert!(periods::valuation_report(&db, None, "colour".to_string())
        .await
        .is_err());
}

// ── ABC by annual consumption value ─────────────────────────────────────────────────────────

#[tokio::test]
async fn abc_ranks_by_consumption_value_not_quantity_or_receipts() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let cheap = article(&db, "INV-ABC-CHEAP", Tracking::default(), None).await;
    let dear = article(&db, "INV-ABC-DEAR", Tracking::default(), None).await;
    let idle = article(&db, "INV-ABC-IDLE", Tracking::default(), None).await;
    stock_in(&db, cheap, location_id, 500.0, 1.0).await; // big receipt, not consumption
    stock_in(&db, dear, location_id, 5.0, 100.0).await;
    stock_in(&db, idle, location_id, 1000.0, 50.0).await; // high stock value, never used
    let wo_id = work_order(&db, "OT-ABC-0001", "in_progress").await;
    issue::issue_stock(&db, issue_request(cheap, location_id, 10.0, wo_id))
        .await
        .expect("cheap issue"); // 10 × 1 = 10
    issue::issue_stock(&db, issue_request(dear, location_id, 1.0, wo_id))
        .await
        .expect("dear issue"); // 1 × 100 = 100

    assert!(queries::calculate_abc_classification(&db).await.expect("abc") >= 3);
    let class = |id: i64| {
        let db = &db;
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT abc_class_code FROM articles WHERE id = ?",
                [id.into()],
            ))
            .await
            .expect("query")
            .expect("article")
            .try_get::<Option<String>>("", "abc_class_code")
            .expect("class")
        }
    };
    assert_eq!(class(dear).await.as_deref(), Some("A"));
    assert_eq!(class(cheap).await.as_deref(), Some("B"));
    assert_eq!(class(idle).await.as_deref(), Some("C"));
}

// ── Approval limits and separation of duties; multi-line requisitions ────────────────────────

#[tokio::test]
async fn approvals_enforce_separation_of_duties_and_role_limits() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let a = article(&db, "INV-APR-A", Tracking::default(), None).await;
    let b = article(&db, "INV-APR-B", Tracking::default(), None).await;
    let requester = user(&db, "req_user").await;
    let approver = user(&db, "approver_user").await;
    let outsider = user(&db, "outsider_user").await;
    let buyer_role = role_for(&db, "inv_approver_role", approver).await;

    let req = procurement::create_multi_line_requisition(
        &db,
        CreateMultiLineRequisitionInput {
            lines: vec![
                RequisitionLineInput {
                    article_id: a,
                    preferred_location_id: Some(location_id),
                    requested_qty: 5.0,
                    estimated_unit_cost: Some(10.0),
                },
                RequisitionLineInput {
                    article_id: b,
                    preferred_location_id: Some(location_id),
                    requested_qty: 2.0,
                    estimated_unit_cost: Some(10.0),
                },
            ],
            demand_source_type: "MANUAL".to_string(),
            demand_source_ref: None,
            purchase_priority: None,
            reason: Some("pump overhaul kit".to_string()),
            actor_id: Some(requester),
        },
    )
    .await
    .expect("multi-line requisition");
    assert_eq!(req.line_count, 2);
    assert!(close(req.estimated_total.expect("total"), 70.0));
    assert!(req.estimate_complete);
    let req = transition_req(&db, &req, "SUBMITTED", Some(requester))
        .await
        .expect("submit");

    let own = transition_req(&db, &req, "APPROVED", Some(requester))
        .await
        .expect_err("self approval");
    assert!(message(&own).contains("GATE_PROC_SELF_APPROVAL"), "{own:?}");
    assert!(
        transition_req(&db, &req, "APPROVED", None).await.is_err(),
        "approval needs an approver"
    );

    approvals::upsert_limit(
        &db,
        ProcurementApprovalLimitInput {
            role_id: buyer_role,
            document_type: approvals::DOC_REQUISITION.to_string(),
            max_amount: Some(50.0),
            is_active: Some(true),
            actor_id: None,
        },
    )
    .await
    .expect("limit 50");
    let over = transition_req(&db, &req, "APPROVED", Some(approver))
        .await
        .expect_err("over limit");
    assert!(message(&over).contains("GATE_PROC_APPROVAL_LIMIT"), "{over:?}");
    let none = transition_req(&db, &req, "APPROVED", Some(outsider))
        .await
        .expect_err("no limit");
    assert!(message(&none).contains("GATE_PROC_NO_APPROVAL_LIMIT"), "{none:?}");

    let raised = approvals::upsert_limit(
        &db,
        ProcurementApprovalLimitInput {
            role_id: buyer_role,
            document_type: approvals::DOC_REQUISITION.to_string(),
            max_amount: Some(100.0),
            is_active: Some(true),
            actor_id: None,
        },
    )
    .await
    .expect("limit 100");
    assert_eq!(
        approvals::list_limits(&db).await.expect("limits").len(),
        1,
        "one limit per role and type"
    );
    assert!(close(raised.max_amount.expect("max"), 100.0));
    let req = transition_req(&db, &req, "APPROVED", Some(approver))
        .await
        .expect("approved");
    assert_eq!(req.status, "APPROVED");

    // The PO carries every line, priced from the estimates (tenant currency), and is received
    // line by line with partial receipts.
    let po = procurement::create_purchase_order_from_requisition(
        &db,
        CreatePurchaseOrderFromRequisitionInput {
            requisition_id: req.id,
            supplier_id: None,
            supplier_company_id: None,
            actor_id: Some(outsider),
            currency_code: None,
        },
    )
    .await
    .expect("po");
    assert_eq!(po.line_count, 2);
    assert!(close(po.total_amount.expect("po total"), 70.0));
    let po = transition_po(&db, &po, "SUBMITTED", Some(outsider))
        .await
        .expect("submit po");
    let own_po = transition_po(&db, &po, "APPROVED", Some(outsider))
        .await
        .expect_err("buyer approves own PO");
    assert!(message(&own_po).contains("GATE_PROC_SELF_APPROVAL"));
    let po = transition_po(&db, &po, "APPROVED", Some(approver))
        .await
        .expect("approve po");

    let lines = procurement::list_purchase_order_lines(&db, po.id).await.expect("lines");
    let line_a = lines.iter().find(|l| l.article_id == a).expect("line a");
    procurement::receive_purchase_order_goods(
        &db,
        ReceiveGoodsInput {
            purchase_order_id: po.id,
            lines: vec![receipt_line(line_a.id, a, location_id, 3.0)],
            fulfillment_action: None,
            actor_id: Some(approver),
            exchange_rate: None,
            posting_date: None,
        },
    )
    .await
    .expect("partial receipt");
    let detail = procurement::get_purchase_order_detail(&db, po.id)
        .await
        .expect("detail");
    assert_eq!(detail.order.status, "PARTIALLY_RECEIVED");
    let line_a = detail.lines.iter().find(|l| l.article_id == a).expect("line a");
    assert!(close(line_a.remaining_qty, 2.0));
    let (_, _, avg) = on_hand_and_reserved(&db, a, location_id).await;
    assert!(close(avg.expect("avg"), 10.0), "received at the PO price");
}

#[tokio::test]
async fn draft_requisition_lines_can_be_replaced_and_empty_ones_are_not_submitted() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let a = article(&db, "INV-RPL-A", Tracking::default(), None).await;
    let req = procurement::create_multi_line_requisition(
        &db,
        CreateMultiLineRequisitionInput {
            lines: vec![RequisitionLineInput {
                article_id: a,
                preferred_location_id: Some(location_id),
                requested_qty: 1.0,
                estimated_unit_cost: None,
            }],
            demand_source_type: "MANUAL".to_string(),
            demand_source_ref: None,
            purchase_priority: None,
            reason: None,
            actor_id: None,
        },
    )
    .await
    .expect("requisition");
    assert!(!req.estimate_complete);
    let replaced = procurement::replace_requisition_lines(
        &db,
        crate::inventory::domain::ReplaceRequisitionLinesInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            lines: vec![RequisitionLineInput {
                article_id: a,
                preferred_location_id: Some(location_id),
                requested_qty: 4.0,
                estimated_unit_cost: Some(2.5),
            }],
            actor_id: None,
        },
    )
    .await
    .expect("replace lines");
    assert!(close(replaced.estimated_total.expect("total"), 10.0));
    // A stale row version is refused.
    assert!(procurement::replace_requisition_lines(
        &db,
        crate::inventory::domain::ReplaceRequisitionLinesInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            lines: vec![],
            actor_id: None,
        },
    )
    .await
    .is_err());
    // A requisition without lines cannot be created.
    assert!(procurement::create_multi_line_requisition(
        &db,
        CreateMultiLineRequisitionInput {
            lines: vec![],
            demand_source_type: "MANUAL".to_string(),
            demand_source_ref: None,
            purchase_priority: None,
            reason: None,
            actor_id: None,
        },
    )
    .await
    .is_err());
}

// ── Supplier currency, rate captured at receipt ──────────────────────────────────────────────

#[tokio::test]
async fn foreign_currency_po_is_valued_in_tenant_currency_at_the_receipt_rate() {
    let db = setup_db().await;
    let location_id = main_location_id(&db).await;
    let article_id = article(&db, "INV-FX-001", Tracking::default(), None).await;
    let requester = user(&db, "fx_requester").await;
    let approver = user(&db, "fx_approver").await;
    let tenant = fx::tenant_currency(&db).await.expect("tenant currency");
    assert_ne!(tenant, "EUR");

    let req = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id,
            preferred_location_id: Some(location_id),
            requested_qty: 4.0,
            demand_source_type: "MANUAL".to_string(),
            demand_source_id: None,
            demand_source_ref: None,
            demand_source_line_id: None,
            source_reservation_id: None,
            source_reorder_trigger: None,
            purchase_priority: None,
            reason: None,
            actor_id: Some(requester),
        },
    )
    .await
    .expect("requisition");
    let req = transition_req(&db, &req, "SUBMITTED", Some(requester))
        .await
        .expect("submit");
    let req = transition_req(&db, &req, "APPROVED", Some(approver))
        .await
        .expect("approve");
    let po = procurement::create_purchase_order_from_requisition(
        &db,
        CreatePurchaseOrderFromRequisitionInput {
            requisition_id: req.id,
            supplier_id: None,
            supplier_company_id: None,
            actor_id: Some(requester),
            currency_code: Some("eur".to_string()),
        },
    )
    .await
    .expect("po in EUR");
    assert_eq!(po.currency_code.as_deref(), Some("EUR"));
    let lines = procurement::list_purchase_order_lines(&db, po.id).await.expect("lines");
    assert!(
        lines[0].unit_price.is_none(),
        "a foreign-currency PO is priced by the buyer"
    );
    procurement::set_purchase_order_line_price(&db, lines[0].id, Some(10.0))
        .await
        .expect("price in EUR");
    let po = procurement::get_purchase_order_detail(&db, po.id)
        .await
        .expect("detail")
        .order;
    let po = transition_po(&db, &po, "SUBMITTED", Some(requester))
        .await
        .expect("submit po");
    let po = transition_po(&db, &po, "APPROVED", Some(approver))
        .await
        .expect("approve po");
    assert!(
        procurement::set_purchase_order_line_price(&db, lines[0].id, Some(11.0))
            .await
            .is_err(),
        "prices are frozen after the draft"
    );

    let receive = |qty: f64, rate: Option<f64>| ReceiveGoodsInput {
        purchase_order_id: po.id,
        lines: vec![receipt_line(lines[0].id, article_id, location_id, qty)],
        fulfillment_action: None,
        actor_id: Some(approver),
        exchange_rate: rate,
        posting_date: None,
    };
    let missing = procurement::receive_purchase_order_goods(&db, receive(2.0, None))
        .await
        .expect_err("no rate");
    assert!(message(&missing).contains("GATE_PROC_NO_EXCHANGE_RATE"), "{missing:?}");

    let gr = procurement::receive_purchase_order_goods(&db, receive(2.0, Some(150.0)))
        .await
        .expect("receipt at the entered rate");
    assert_eq!(gr.exchange_rate_source.as_deref(), Some(fx::RATE_SOURCE_ENTERED));
    assert!(close(gr.exchange_rate.expect("rate"), 150.0));
    let gr_lines = procurement::list_goods_receipt_lines(&db, gr.id)
        .await
        .expect("gr lines");
    assert!(close(gr_lines[0].unit_cost_tenant.expect("tenant cost"), 1500.0));
    let (_, _, avg) = on_hand_and_reserved(&db, article_id, location_id).await;
    assert!(close(avg.expect("avg"), 1500.0));

    fx::create_rate(
        &db,
        CurrencyExchangeRateInput {
            from_currency: "EUR".to_string(),
            to_currency: None,
            rate: 140.0,
            valid_from: fx::today(),
            source: Some("bank".to_string()),
        },
        Some(approver),
    )
    .await
    .expect("stored rate");
    let gr2 = procurement::receive_purchase_order_goods(&db, receive(2.0, None))
        .await
        .expect("receipt at the stored rate");
    assert_eq!(gr2.exchange_rate_source.as_deref(), Some(fx::RATE_SOURCE_TABLE));
    let (on_hand, _, avg) = on_hand_and_reserved(&db, article_id, location_id).await;
    assert!(close(on_hand, 4.0));
    assert!(close(avg.expect("avg"), 1450.0), "(2 × 1500 + 2 × 1400) / 4");

    let resolved = fx::resolve_rate(&db, "EUR".to_string(), None).await.expect("resolve");
    assert!(close(resolved.rate.expect("rate"), 140.0));
    assert!(
        fx::create_rate(
            &db,
            CurrencyExchangeRateInput {
                from_currency: "EUR".to_string(),
                to_currency: None,
                rate: 141.0,
                valid_from: fx::today(),
                source: None,
            },
            None,
        )
        .await
        .is_err(),
        "one active rate per pair and date"
    );
}
