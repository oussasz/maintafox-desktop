//! L3-04: a hors-stock part bought for a work order. It is asked for on the work order (an
//! existing hors-stock article reused, or one created "à revoir"), approved and ordered as usual,
//! received with no stock movement and booked to the work order as parts actual; the work order
//! does not close while the purchase is open.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::inventory::domain::{
    CreateProcurementRequisitionInput, CreatePurchaseOrderFromRequisitionInput, ProcurementRequisition, PurchaseOrder,
    ReceiveGoodsInput, ReceivePurchaseOrderLineInput, TransitionProcurementRequisitionInput,
    TransitionPurchaseOrderInput,
};
use crate::inventory::non_stock::{self, CreateWoNonStockPurchaseInput, ReviewNonStockArticleInput};
use crate::inventory::procurement;

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

fn message(err: &AppError) -> String {
    format!("{err:?}")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
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
        vec![format!("hs-user-{username}").into(), username.into(), username.into()],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await
}

/// A user holding the seeded Administrator role (approval right, unlimited ceiling).
async fn administrator(db: &DatabaseConnection, username: &str) -> i64 {
    let user_id = user(db, username).await;
    exec(
        db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
             valid_from, valid_to, assigned_by_id, notes, created_at, updated_at)
         SELECT ?, ?, id, 'tenant', NULL, NULL, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM roles WHERE name = 'Administrator'",
        vec![format!("hs-usa-{username}").into(), user_id.into()],
    )
    .await;
    user_id
}

async fn main_location_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT id FROM stock_locations WHERE code = 'MAIN-BIN' LIMIT 1",
        vec![],
    )
    .await
}

async fn unit_id(db: &DatabaseConnection) -> i64 {
    crate::reference::units::unit_id_by_code(db, "U").await.expect("unit U")
}

async fn transition_req(
    db: &DatabaseConnection,
    req: &ProcurementRequisition,
    next: &str,
    actor: i64,
) -> ProcurementRequisition {
    procurement::transition_procurement_requisition(
        db,
        TransitionProcurementRequisitionInput {
            requisition_id: req.id,
            expected_row_version: req.row_version,
            next_status: next.to_string(),
            reason: Some("Plus nécessaire pour cet OT".to_string()),
            note: None,
            actor_id: Some(actor),
        },
    )
    .await
    .expect("requisition transition")
}

async fn transition_po(db: &DatabaseConnection, po: &PurchaseOrder, next: &str, actor: i64) -> PurchaseOrder {
    procurement::transition_purchase_order(
        db,
        TransitionPurchaseOrderInput {
            purchase_order_id: po.id,
            expected_row_version: po.row_version,
            next_status: next.to_string(),
            reason: None,
            note: None,
            actor_id: Some(actor),
        },
    )
    .await
    .expect("purchase order transition")
}

fn request(wo_id: i64, name: &str, unit: i64, qty: f64, estimate: Option<f64>) -> CreateWoNonStockPurchaseInput {
    CreateWoNonStockPurchaseInput {
        work_order_id: wo_id,
        article_id: None,
        article_name: Some(name.to_string()),
        unit_value_id: Some(unit),
        quantity: qty,
        estimated_unit_cost: estimate,
        notes: None,
        reason: Some("Pièce introuvable en magasin".to_string()),
    }
}

/// Approves the requisition, orders it at the estimate and approves the order.
async fn order(db: &DatabaseConnection, req: &ProcurementRequisition, requester: i64, approver: i64) -> PurchaseOrder {
    let req = transition_req(db, req, "SUBMITTED", requester).await;
    let req = transition_req(db, &req, "APPROVED", approver).await;
    let po = procurement::create_purchase_order_from_requisition(
        db,
        CreatePurchaseOrderFromRequisitionInput {
            requisition_id: req.id,
            supplier_id: None,
            supplier_company_id: None,
            actor_id: Some(requester),
            currency_code: None,
        },
    )
    .await
    .expect("purchase order");
    let po = transition_po(db, &po, "SUBMITTED", requester).await;
    transition_po(db, &po, "APPROVED", approver).await
}

async fn receive(db: &DatabaseConnection, po: &PurchaseOrder, article_id: i64, qty: f64, actor: i64) {
    let lines = procurement::list_purchase_order_lines(db, po.id).await.expect("lines");
    procurement::receive_purchase_order_goods(
        db,
        ReceiveGoodsInput {
            purchase_order_id: po.id,
            lines: vec![ReceivePurchaseOrderLineInput {
                po_line_id: lines[0].id,
                article_id,
                location_id: main_location_id(db).await,
                received_qty: qty,
                accepted_qty: qty,
                rejected_qty: 0.0,
                rejection_reason: None,
                lots: None,
                entered_unit_value_id: None,
            }],
            fulfillment_action: Some("ISSUE_TO_DEMAND".to_string()),
            actor_id: Some(actor),
            exchange_rate: None,
            posting_date: None,
        },
    )
    .await
    .expect("receipt");
}

#[tokio::test]
async fn a_hors_stock_part_is_received_without_stock_and_booked_to_its_work_order() {
    let db = crate::db::test_db::seeded().await;
    let wo_id = work_order(&db, "OT-HS-0001", "in_progress").await;
    let requester = user(&db, "hs_requester").await;
    let approver = administrator(&db, "hs_approver").await;
    let unit = unit_id(&db).await;

    let asked = non_stock::create_wo_non_stock_purchase(
        &db,
        request(wo_id, "Joint torique 212", unit, 3.0, Some(40.0)),
        requester,
    )
    .await
    .expect("ask for the part");
    let article = &asked.article;
    assert!(article.article_code.starts_with("HS-"), "{}", article.article_code);
    assert_eq!(article.catalogue_review_state.as_deref(), Some("TO_REVIEW"));
    assert_eq!(article.origin_work_order_id, Some(wo_id));
    assert!(asked.part.is_non_stock);
    assert_eq!(asked.part.consumption_status, "pending");
    assert_eq!(asked.part.quantity_used, None, "nothing used before the receipt");
    assert_eq!(asked.requisition.demand_source_type, "WORK_ORDER_PART");
    assert_eq!(asked.requisition.demand_source_id, Some(wo_id));
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT demand_source_line_id FROM procurement_requisition_lines WHERE requisition_id = ?",
            vec![asked.requisition.id.into()],
        )
        .await,
        asked.part.id,
        "the requisition points at the work-order line"
    );
    assert_eq!(non_stock::open_purchase_line_count(&db, wo_id).await.expect("open"), 1);

    let po = order(&db, &asked.requisition, requester, approver).await;
    receive(&db, &po, article.id, 2.0, approver).await;

    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM stock_balances WHERE article_id = ?",
            vec![article.id.into()]
        )
        .await,
        0,
        "nothing entered the store"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM inventory_transactions WHERE article_id = ?",
            vec![article.id.into()]
        )
        .await,
        0,
        "no stock movement"
    );
    let part = crate::wo::parts::load_part_in(&db, asked.part.id).await.expect("part");
    assert!(close(part.non_stock_received_qty, 2.0));
    assert_eq!(part.quantity_used, Some(2.0));
    assert!(close(part.unit_cost.expect("cost"), 40.0), "booked at the order price");
    assert_eq!(part.consumption_status, "used");
    let costs = crate::finance::wo_costing::compute_wo_cost_breakdown(&db, wo_id)
        .await
        .expect("costs");
    assert!(
        close(costs.parts.actual, 80.0),
        "2 × 40 in parts actual: {}",
        costs.parts.actual
    );
    assert_eq!(
        non_stock::open_purchase_line_count(&db, wo_id).await.expect("open"),
        1,
        "one still to receive"
    );

    receive(&db, &po, article.id, 1.0, approver).await;
    let part = crate::wo::parts::load_part_in(&db, asked.part.id).await.expect("part");
    assert!(close(part.non_stock_received_qty, 3.0));
    let costs = crate::finance::wo_costing::compute_wo_cost_breakdown(&db, wo_id)
        .await
        .expect("costs");
    assert!(close(costs.parts.actual, 120.0), "{}", costs.parts.actual);
    assert_eq!(non_stock::open_purchase_line_count(&db, wo_id).await.expect("open"), 0);

    let unknown = non_stock::set_part_handed_to(&db, part.id, Some(987_654))
        .await
        .expect_err("unknown person");
    assert!(
        message(&unknown).contains("GATE_INV_NON_STOCK_PERSON_UNKNOWN"),
        "{unknown:?}"
    );
    let technician = person(&db, "T-HS-01").await;
    let handed = non_stock::set_part_handed_to(&db, part.id, Some(technician))
        .await
        .expect("handed to");
    assert_eq!(handed.handed_to_personnel_id, Some(technician));
}

async fn person(db: &DatabaseConnection, code: &str) -> i64 {
    exec(
        db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES (?, 'Karim Benali', 'available')",
        vec![code.into()],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM personnel WHERE employee_code = ?",
        vec![code.into()],
    )
    .await
}

async fn unused_reason_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(
        db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.PART_UNUSED_REASON' AND rv.is_active = 1 \
           AND rs.status = 'published' AND LOWER(rv.code) <> 'other' LIMIT 1",
        vec![],
    )
    .await
}

/// Review finding: a hors-stock line asked for before the work starts is a planned line; when its
/// purchase ends with nothing received it is marked not used, so the work order can complete.
#[tokio::test]
async fn a_planned_hors_stock_line_whose_purchase_is_cancelled_is_marked_not_used() {
    let db = crate::db::test_db::seeded().await;
    let wo_id = work_order(&db, "OT-HS-0005", "ready_to_schedule").await;
    let requester = user(&db, "hs_req5").await;
    let unit = unit_id(&db).await;
    let asked = non_stock::create_wo_non_stock_purchase(
        &db,
        request(wo_id, "Garniture mécanique 45 mm", unit, 1.0, Some(300.0)),
        requester,
    )
    .await
    .expect("ask");
    assert_eq!(asked.part.origin, "planned");
    exec(
        &db,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress') \
         WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    let reason = unused_reason_id(&db).await;
    let input = || crate::wo::parts::MarkPartNotUsedInput {
        wo_part_id: asked.part.id,
        not_used_reason_id: reason,
        not_used_comment: None,
        actor_id: Some(requester),
    };

    let open = crate::wo::parts::mark_part_not_used(&db, input())
        .await
        .expect_err("purchase still open");
    assert!(
        message(&open).contains("WO_PART_LINE_NON_STOCK_PURCHASE_OPEN"),
        "{open:?}"
    );

    transition_req(&db, &asked.requisition, "CANCELLED", requester).await;
    let settled = crate::wo::parts::mark_part_not_used(&db, input())
        .await
        .expect("not used once the purchase is cancelled");
    assert_eq!(settled.consumption_status, "not_used");
}

/// Review finding: a hors-stock article never enters the store, and its purchase books only onto
/// its own work-order line.
#[tokio::test]
async fn a_hors_stock_article_cannot_be_put_into_the_store() {
    let db = crate::db::test_db::seeded().await;
    let wo_id = work_order(&db, "OT-HS-0006", "in_progress").await;
    let requester = user(&db, "hs_req6").await;
    let unit = unit_id(&db).await;
    let asked = non_stock::create_wo_non_stock_purchase(
        &db,
        request(wo_id, "Flexible hydraulique 2 m", unit, 1.0, None),
        requester,
    )
    .await
    .expect("ask");
    let location = main_location_id(&db).await;

    let adjust = crate::inventory::queries::adjust_stock_full(
        &db,
        crate::inventory::domain::InventoryStockAdjustInput {
            article_id: asked.article.id,
            location_id: location,
            delta_qty: 1.0,
            reason_code: Some("ENTREE_ACHAT".to_string()),
            notes: None,
            source_ref: None,
        },
        Default::default(),
    )
    .await
    .expect_err("no manual entry");
    assert!(
        message(&adjust).contains("GATE_INV_NON_STOCK_NOT_IN_STORE"),
        "{adjust:?}"
    );
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM stock_balances WHERE article_id = ?",
            vec![asked.article.id.into()]
        )
        .await,
        0
    );

    let wrong_line = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id: asked.article.id,
            preferred_location_id: None,
            requested_qty: 1.0,
            demand_source_type: "WORK_ORDER_PART".to_string(),
            demand_source_id: Some(wo_id),
            demand_source_ref: None,
            demand_source_line_id: Some(asked.part.id + 1000),
            source_reservation_id: None,
            source_reorder_trigger: None,
            purchase_priority: None,
            reason: None,
            actor_id: Some(requester),
        },
    )
    .await
    .expect_err("no such line");
    assert!(
        message(&wrong_line).contains("GATE_INV_NON_STOCK_NO_WO_LINE"),
        "{wrong_line:?}"
    );

    let planned = crate::wo::parts::add_planned_part(
        &db,
        crate::wo::parts::AddPartInput {
            wo_id,
            article_id: Some(asked.article.id),
            article_ref: None,
            quantity_planned: 1.0,
            unit_cost: None,
            stock_location_id: None,
            auto_reserve: None,
            notes: None,
            origin: None,
            is_required: true,
        },
        crate::wo::parts::StockAuthority::MayMoveStock,
    )
    .await
    .expect_err("not planned from the store");
    assert!(message(&planned).contains("GATE_INV_NON_STOCK_NEEDS_WO"), "{planned:?}");
}

#[tokio::test]
async fn a_hors_stock_article_is_bought_only_from_a_work_order_and_its_use_is_the_receipt() {
    let db = crate::db::test_db::seeded().await;
    let wo_id = work_order(&db, "OT-HS-0002", "in_progress").await;
    let requester = user(&db, "hs_req2").await;
    let unit = unit_id(&db).await;
    let asked =
        non_stock::create_wo_non_stock_purchase(&db, request(wo_id, "Courroie XPZ 1250", unit, 1.0, None), requester)
            .await
            .expect("ask");

    let manual = procurement::create_procurement_requisition(
        &db,
        CreateProcurementRequisitionInput {
            article_id: asked.article.id,
            preferred_location_id: None,
            requested_qty: 1.0,
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
    .expect_err("no stock requisition for a hors-stock article");
    assert!(message(&manual).contains("GATE_INV_NON_STOCK_NEEDS_WO"), "{manual:?}");

    let by_hand = crate::wo::parts::record_actual_usage(
        &db,
        asked.part.id,
        1.0,
        None,
        crate::wo::parts::StockAuthority::MayMoveStock,
    )
    .await
    .expect_err("the use is the receipt");
    assert!(
        message(&by_hand).contains("WO_PART_LINE_NON_STOCK_BY_RECEIPT"),
        "{by_hand:?}"
    );

    let not_received = non_stock::set_part_handed_to(&db, asked.part.id, Some(requester))
        .await
        .expect_err("not received yet");
    assert!(
        message(&not_received).contains("GATE_INV_NON_STOCK_NOT_HORS_STOCK"),
        "{not_received:?}"
    );
}

#[tokio::test]
async fn an_open_hors_stock_purchase_blocks_the_close_until_it_is_cancelled() {
    let db = crate::db::test_db::seeded().await;
    let wo_id = work_order(&db, "OT-HS-0003", "in_progress").await;
    let requester = user(&db, "hs_req3").await;
    let unit = unit_id(&db).await;
    let asked = non_stock::create_wo_non_stock_purchase(
        &db,
        request(wo_id, "Capteur PT100 spécial", unit, 1.0, Some(90.0)),
        requester,
    )
    .await
    .expect("ask");

    let evidence = crate::wo::closeout::CloseEvidence {
        closer_user_id: Some(requester),
        no_downtime_attestation: None,
        no_downtime_attestation_reason: None,
        fmeca_override_reason: None,
        fmeca_override_signer_id: None,
        fmeca_override_signer_password: crate::wo::closeout::SignerPassword::Given(None),
    };
    let rpn = crate::reliability::advanced_rams::queries::fmeca_rpn_critical_threshold_i64_in(&db)
        .await
        .expect("rpn");
    let refusals = crate::wo::closeout::close_preflight(&db, wo_id, &evidence, rpn)
        .await
        .expect("preflight")
        .0;
    assert!(
        refusals
            .iter()
            .any(|r| r.starts_with("GATE_CLOSEOUT_NON_STOCK_OPEN:count=1")),
        "{refusals:?}"
    );

    transition_req(&db, &asked.requisition, "CANCELLED", requester).await;
    assert_eq!(non_stock::open_purchase_line_count(&db, wo_id).await.expect("open"), 0);
    let refusals = crate::wo::closeout::close_preflight(&db, wo_id, &evidence, rpn)
        .await
        .expect("preflight")
        .0;
    assert!(
        !refusals.iter().any(|r| r.starts_with("GATE_CLOSEOUT_NON_STOCK_OPEN")),
        "{refusals:?}"
    );
}

#[tokio::test]
async fn hors_stock_articles_are_reused_reviewed_and_never_deleted() {
    let db = crate::db::test_db::seeded().await;
    let wo_id = work_order(&db, "OT-HS-0004", "planned").await;
    let requester = user(&db, "hs_req4").await;
    let manager = administrator(&db, "hs_manager").await;
    let unit = unit_id(&db).await;

    let first = non_stock::create_wo_non_stock_purchase(
        &db,
        request(wo_id, "Roulement 6205-2RS spécial", unit, 2.0, None),
        requester,
    )
    .await
    .expect("first");
    assert_eq!(first.part.origin, "planned", "asked before execution");
    let found = non_stock::search_non_stock_articles(&db, Some("6205".to_string()))
        .await
        .expect("search");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, first.article.id);

    let mut reuse = request(wo_id, "", unit, 1.0, None);
    reuse.article_id = Some(first.article.id);
    reuse.article_name = None;
    let second = non_stock::create_wo_non_stock_purchase(&db, reuse.clone(), requester)
        .await
        .expect("reuse");
    assert_eq!(second.article.id, first.article.id, "no second article");

    let other =
        non_stock::create_wo_non_stock_purchase(&db, request(wo_id, "Roulement 6205 2RS", unit, 1.0, None), requester)
            .await
            .expect("other");
    let marked = non_stock::review_non_stock_article(
        &db,
        ReviewNonStockArticleInput {
            article_id: other.article.id,
            action: "DUPLICATE".to_string(),
            duplicate_of_article_id: Some(first.article.id),
        },
        manager,
    )
    .await
    .expect("duplicate");
    assert_eq!(marked.catalogue_review_state.as_deref(), Some("DUPLICATE"));
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM articles WHERE id = ?",
            vec![other.article.id.into()]
        )
        .await,
        1,
        "kept"
    );
    let found = non_stock::search_non_stock_articles(&db, Some("6205".to_string()))
        .await
        .expect("search");
    assert_eq!(found.len(), 1, "a duplicate is no longer offered");
    let mut dup = reuse.clone();
    dup.article_id = Some(other.article.id);
    let refused = non_stock::create_wo_non_stock_purchase(&db, dup, requester)
        .await
        .expect_err("duplicate refused");
    assert!(
        message(&refused).contains("GATE_INV_NON_STOCK_DUPLICATE"),
        "{refused:?}"
    );

    // A duplicate is never the target of another duplicate.
    let chained = non_stock::review_non_stock_article(
        &db,
        ReviewNonStockArticleInput {
            article_id: first.article.id,
            action: "DUPLICATE".to_string(),
            duplicate_of_article_id: Some(other.article.id),
        },
        manager,
    )
    .await
    .expect_err("no chain");
    assert!(
        message(&chained).contains("GATE_INV_NON_STOCK_REVIEW_ACTION"),
        "{chained:?}"
    );

    let promote = || ReviewNonStockArticleInput {
        article_id: first.article.id,
        action: "PROMOTE".to_string(),
        duplicate_of_article_id: None,
    };
    // Its work-order purchases are still open: the delivery must go to the work order.
    let refused = non_stock::review_non_stock_article(&db, promote(), manager)
        .await
        .expect_err("purchase open");
    assert!(
        message(&refused).contains("GATE_INV_NON_STOCK_PROMOTE_OPEN_PURCHASE"),
        "{refused:?}"
    );
    transition_req(&db, &first.requisition, "CANCELLED", requester).await;
    transition_req(&db, &second.requisition, "CANCELLED", requester).await;
    non_stock::review_non_stock_article(&db, promote(), manager)
        .await
        .expect("promote");
    assert!(
        !non_stock::is_non_stock_article(&db, first.article.id)
            .await
            .expect("kind"),
        "now a stocked article"
    );
}
