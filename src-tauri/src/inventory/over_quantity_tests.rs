//! "Sur-sortie / sur-réception — à rapprocher" (`inventory::negative_stock`, AU-103, CMMS Advisor
//! decision D12 (a)): two PCs issuing the whole of one reservation, or receiving the same PO line,
//! leave it issued or received beyond its quantity. The item is listed on every PC holding the
//! movements, whatever order they arrived in, never on one PC's normal flows, and is closed only by
//! a count made where every one of its movements was held, with a reason of at least 8 characters.

use sea_orm::DatabaseConnection;

use super::negative_stock_tests::{
    enter_count, local_pair, on_hand_milli, post_count, refusal_code, resolution, stocked_pair, user_with,
};
use crate::inventory::domain::{
    InventoryIssueInput, InventoryReserveInput, InventoryReturnInput, ReceiveGoodsInput, ReceivePurchaseOrderLineInput,
};
use crate::inventory::negative_stock::{
    self, OverQuantityReview, OVER_ISSUE, OVER_RECEIPT, REFUSED_NOT_OPEN, REFUSED_REASON_SHORT,
};
use crate::inventory::{procurement, queries};
use crate::sync::test_support::{exec, scalar_i64, started_device_db, CreatedRow, RowFactory, TestServer};

/// One exchange with the server. The reservation row (or PO line) that both PCs updated is a sync
/// conflict of its own (operator review); the movements are applied all the same, so only apply
/// errors and deferred rows fail the test.
async fn sync(server: &mut TestServer, device: &str, db: &DatabaseConnection) {
    let report = server.exchange(device, db).await;
    assert!(
        report.materialized.errors.is_empty(),
        "{device}: {:?}",
        report.materialized.errors
    );
    assert_eq!(report.materialized.deferred, 0, "{device}: deferred rows");
}

async fn reserve(db: &DatabaseConnection, article_id: i64, location_id: i64, qty: f64) -> i64 {
    queries::reserve_stock(
        db,
        InventoryReserveInput {
            article_id,
            location_id,
            quantity: qty,
            source_type: "MANUAL".into(),
            source_id: None,
            source_ref: Some("RES-AU103".into()),
            notes: None,
        },
    )
    .await
    .expect("reserve")
    .id
}

async fn issue(db: &DatabaseConnection, reservation_id: i64, qty: f64) -> Result<(), crate::errors::AppError> {
    queries::issue_reserved_stock(
        db,
        InventoryIssueInput {
            reservation_id,
            quantity: qty,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .map(|_| ())
}

async fn local_id(db: &DatabaseConnection, table: &str, sync_id: &str) -> i64 {
    scalar_i64(
        db,
        &format!("SELECT id FROM {table} WHERE entity_sync_id = ?"),
        vec![sync_id.into()],
    )
    .await
}

async fn sync_id_of(db: &DatabaseConnection, table: &str, id: i64) -> String {
    crate::sync::test_support::scalar_opt_text(
        db,
        &format!("SELECT entity_sync_id FROM {table} WHERE id = ?"),
        vec![id.into()],
    )
    .await
    .expect("sync id")
}

/// `(on hand, reserved)` of the pair, in thousandths.
async fn balance_milli(db: &DatabaseConnection, article_id: i64, location_id: i64) -> (i64, i64) {
    (
        on_hand_milli(db, article_id, location_id).await,
        scalar_i64(
            db,
            "SELECT CAST(ROUND(reserved_qty * 1000) AS INTEGER) FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![article_id.into(), location_id.into()],
        )
        .await,
    )
}

/// The open over-issues and over-receipts of the pair, and the closed ones.
async fn over_items(
    db: &DatabaseConnection,
    article_id: i64,
    location_id: i64,
) -> (Vec<OverQuantityReview>, Vec<OverQuantityReview>) {
    let list = negative_stock::list_reviews(db, true).await.expect("list");
    let of_pair = |items: Vec<OverQuantityReview>| -> Vec<OverQuantityReview> {
        items
            .into_iter()
            .filter(|i| i.article_id == article_id && i.location_id == location_id)
            .collect()
    };
    (of_pair(list.over_quantities), of_pair(list.closed_over_quantities))
}

fn movement_ids(item: &OverQuantityReview) -> Vec<String> {
    let mut ids: Vec<String> = item.movements.iter().map(|m| m.movement_sync_id.clone()).collect();
    ids.sort();
    ids
}

/// Stock 10 at a location known to A, B and C, with one reservation of 5 made on A and received by
/// all three. Returns the PCs, the server, each PC's `(article, location, reservation)`.
struct SharedReservation {
    dbs: [DatabaseConnection; 3],
    server: TestServer,
    ids: [(i64, i64, i64); 3],
}

async fn shared_reservation() -> SharedReservation {
    let dbs = [
        started_device_db().await,
        started_device_db().await,
        started_device_db().await,
    ];
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location): (CreatedRow, CreatedRow) = stocked_pair(&dbs[0], &mut factory, 10.0).await;
    let reservation = reserve(&dbs[0], article.rowid, location.rowid, 5.0).await;
    let reservation_sync_id = sync_id_of(&dbs[0], "stock_reservations", reservation).await;
    for (name, db) in ["A", "B", "C"].into_iter().zip(&dbs) {
        sync(&mut server, name, db).await;
    }
    let mut ids = [(0, 0, 0); 3];
    for (i, db) in dbs.iter().enumerate() {
        let (art, loc) = local_pair(db, &article, &location).await;
        ids[i] = (art, loc, local_id(db, "stock_reservations", &reservation_sync_id).await);
    }
    SharedReservation { dbs, server, ids }
}

/// A and B each issue the whole reservation of 5 offline. A receives B's issue after its own, B
/// receives A's after its own, C receives B's first and A's later: on every PC one over-issue of
/// the same two movements (reserved 5, issued 10, excess 5), counted in the sync centre summary,
/// with the reservation's reference; the stock stands at 0 and the reserved stock at 0, never
/// below (AU-103 part 1).
#[tokio::test]
async fn an_over_issue_from_two_pcs_is_listed_on_every_pc_whatever_the_arrival_order() {
    let SharedReservation {
        dbs: [a, b, c],
        mut server,
        ids,
    } = shared_reservation().await;
    issue(&a, ids[0].2, 5.0).await.expect("A issues the reservation");
    issue(&b, ids[1].2, 5.0).await.expect("B issues the reservation");
    for (pc, db, (art, loc, _)) in [("A", &a, ids[0]), ("B", &b, ids[1])] {
        let (open, _) = over_items(db, art, loc).await;
        assert!(open.is_empty(), "{pc}: its own issue alone is no over-issue");
    }
    sync(&mut server, "B", &b).await;
    sync(&mut server, "C", &c).await;
    assert!(
        over_items(&c, ids[2].0, ids[2].1).await.0.is_empty(),
        "C: B's issue alone is no over-issue"
    );
    sync(&mut server, "A", &a).await;
    sync(&mut server, "B", &b).await;
    sync(&mut server, "C", &c).await;

    let mut seen = Vec::new();
    for (pc, db, (art, loc, _)) in [("A", &a, ids[0]), ("B", &b, ids[1]), ("C", &c, ids[2])] {
        assert_eq!(
            balance_milli(db, art, loc).await,
            (0, 0),
            "{pc}: 10 − 5 − 5, nothing reserved"
        );
        let (open, closed) = over_items(db, art, loc).await;
        assert_eq!(open.len(), 1, "{pc}: one over-issue");
        assert!(closed.is_empty(), "{pc}");
        let item = &open[0];
        assert_eq!(item.kind, OVER_ISSUE, "{pc}");
        assert_eq!(
            (item.expected_qty, item.actual_qty, item.excess_qty),
            (5.0, 10.0, 5.0),
            "{pc}"
        );
        assert_eq!(item.reservation_ref.as_deref(), Some("RES-AU103"), "{pc}");
        assert_eq!(item.purchase_order_number, None, "{pc}");
        assert_eq!(item.movements.len(), 2, "{pc}");
        assert!(item.movements.iter().all(|m| m.movement_type == "ISSUE"), "{pc}");
        assert_eq!(item.closed_by, None, "{pc}");
        let summary = negative_stock::summary(db).await.expect("summary");
        assert_eq!(summary.over_quantity_count, 1, "{pc}");
        assert_eq!(summary.negative_count, 0, "{pc}: the location is at 0, not below");
        seen.push(movement_ids(item));
    }
    assert!(
        seen.windows(2).all(|w| w[0] == w[1]),
        "the same movements on every PC: {seen:?}"
    );
}

/// The over-issue is closed only by a count made on a PC holding both issues: C counts the location
/// (a cycle count) while it holds B's issue only, so the item stays open on every PC once C's count
/// arrives; a short reason is refused; the manager's count on A, which holds both, closes it on A
/// and, once received, on B and C, where it is listed with the closed items.
#[tokio::test]
async fn an_over_issue_is_closed_only_by_a_count_made_where_every_movement_was_held() {
    let SharedReservation {
        dbs: [a, b, c],
        mut server,
        ids,
    } = shared_reservation().await;
    let manager = user_with(&a, "overissuemgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    let counter = user_with(&c, "overissuecounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    issue(&a, ids[0].2, 5.0).await.expect("A issues");
    issue(&b, ids[1].2, 5.0).await.expect("B issues");
    sync(&mut server, "B", &b).await;
    sync(&mut server, "C", &c).await;
    // C holds B's issue only (on hand 5) and counts the 5 it sees: no variance.
    let (art_c, loc_c, _) = ids[2];
    post_count(&c, counter, enter_count(&c, counter, art_c, loc_c, 5.0).await).await;
    for (name, db) in [("A", &a), ("C", &c), ("B", &b), ("A", &a), ("C", &c)] {
        sync(&mut server, name, db).await;
    }
    for (pc, db, (art, loc, _)) in [("A", &a, ids[0]), ("B", &b, ids[1]), ("C", &c, ids[2])] {
        let (open, closed) = over_items(db, art, loc).await;
        assert_eq!(
            (open.len(), closed.len()),
            (1, 0),
            "{pc}: C's count did not hold A's issue: still to reconcile"
        );
    }

    let (art_a, loc_a, _) = ids[0];
    let short = negative_stock::resolve_by_count(&a, manager, resolution(art_a, loc_a, 0.0, "court"))
        .await
        .expect_err("a short reason");
    assert_eq!(refusal_code(short), REFUSED_REASON_SHORT);
    assert_eq!(over_items(&a, art_a, loc_a).await.0.len(), 1, "still open");

    let resolved = negative_stock::resolve_by_count(
        &a,
        manager,
        resolution(art_a, loc_a, 0.0, "Bac recompté : 5 sortis deux fois"),
    )
    .await
    .expect("the count resolves the over-issue");
    assert_eq!(resolved.review, None, "no negative item here");
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.negative_stock.counted'
               AND json_extract(details_json, '$.over_quantity') = 1 AND actor_id = ?",
            vec![i64::from(manager).into()],
        )
        .await,
        1,
        "the count is audited as resolving an over-issue"
    );
    let (open, closed) = over_items(&a, art_a, loc_a).await;
    assert!(open.is_empty(), "A: closed by its count");
    assert_eq!(closed.len(), 1);
    let closing_session = closed[0].closed_by.as_ref().expect("closed by").session_code.clone();
    assert_eq!(closing_session, resolved.session_code);
    assert_eq!(
        negative_stock::summary(&a).await.expect("summary").over_quantity_count,
        0
    );
    let again = negative_stock::resolve_by_count(&a, manager, resolution(art_a, loc_a, 0.0, "Recompté encore"))
        .await
        .expect_err("nothing left to reconcile");
    assert_eq!(refusal_code(again), REFUSED_NOT_OPEN);

    for (name, db) in [("A", &a), ("B", &b), ("C", &c)] {
        sync(&mut server, name, db).await;
    }
    for (pc, db, (art, loc, _)) in [("B", &b, ids[1]), ("C", &c, ids[2])] {
        let (open, closed) = over_items(db, art, loc).await;
        assert!(open.is_empty(), "{pc}: A's count held both issues");
        assert_eq!(closed.len(), 1, "{pc}");
        assert_eq!(
            closed[0].closed_by.as_ref().map(|c| c.session_code.clone()),
            Some(closing_session.clone()),
            "{pc}"
        );
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").over_quantity_count,
            0,
            "{pc}"
        );
    }
}

/// A PO line of 5 on the PC that made it, received by A and B alike: `(PCs, server, each PC's
/// (article, location, PO id, PO line id), the PO number)`.
struct SharedOrder {
    a: DatabaseConnection,
    b: DatabaseConnection,
    server: TestServer,
    ids: [(i64, i64, i64, i64); 2],
    po_number: String,
}

async fn shared_order() -> SharedOrder {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 1.0).await;
    let line = factory.create(&a, "purchase_order_lines").await.expect("PO line");
    exec(
        &a,
        "UPDATE purchase_order_lines SET article_id = ?, ordered_qty = 5, received_qty = 0, status = 'OPEN',
                unit_price = 10, source_reservation_id = NULL
         WHERE id = ?",
        vec![article.rowid.into(), line.rowid.into()],
    )
    .await;
    exec(
        &a,
        "UPDATE purchase_orders SET status = 'APPROVED', currency_code = NULL
         WHERE id = (SELECT purchase_order_id FROM purchase_order_lines WHERE id = ?)",
        vec![line.rowid.into()],
    )
    .await;
    let po_number = crate::sync::test_support::scalar_opt_text(
        &a,
        "SELECT po.po_number FROM purchase_orders po JOIN purchase_order_lines l ON l.purchase_order_id = po.id
         WHERE l.id = ?",
        vec![line.rowid.into()],
    )
    .await
    .expect("PO number");
    sync(&mut server, "A", &a).await;
    sync(&mut server, "B", &b).await;
    let mut ids = [(0, 0, 0, 0); 2];
    for (i, db) in [&a, &b].into_iter().enumerate() {
        let (art, loc) = local_pair(db, &article, &location).await;
        let line_id = local_id(db, "purchase_order_lines", &line.entity_sync_id).await;
        let po_id = scalar_i64(
            db,
            "SELECT purchase_order_id FROM purchase_order_lines WHERE id = ?",
            vec![line_id.into()],
        )
        .await;
        ids[i] = (art, loc, po_id, line_id);
    }
    SharedOrder {
        a,
        b,
        server,
        ids,
        po_number,
    }
}

async fn receive(
    db: &DatabaseConnection,
    (article_id, location_id, po_id, line_id): (i64, i64, i64, i64),
    received: f64,
    rejected: f64,
) -> Result<(), crate::errors::AppError> {
    // Receipt numbers are made from the time in milliseconds: two receipts never share one.
    std::thread::sleep(std::time::Duration::from_millis(3));
    procurement::receive_purchase_order_goods(
        db,
        ReceiveGoodsInput {
            purchase_order_id: po_id,
            lines: vec![ReceivePurchaseOrderLineInput {
                po_line_id: line_id,
                article_id,
                location_id,
                received_qty: received,
                accepted_qty: received - rejected,
                rejected_qty: rejected,
                rejection_reason: (rejected > 0.0).then(|| "abîmé".to_string()),
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
    .map(|_| ())
}

/// A and B each receive the whole PO line of 5 into the same location: on both, whichever receipt
/// arrived second, one over-receipt (ordered 5, received 10, excess 5) naming the PO and both
/// receipt entries, with the stock raised twice (1 + 5 + 5); B's count, holding both, closes it on
/// B and then on A.
#[tokio::test]
async fn an_over_receipt_from_two_pcs_is_listed_on_both_and_a_count_closes_it() {
    let SharedOrder {
        a,
        b,
        mut server,
        ids,
        po_number,
    } = shared_order().await;
    receive(&a, ids[0], 5.0, 0.0).await.expect("A receives the line");
    receive(&b, ids[1], 5.0, 0.0).await.expect("B receives the line");
    for (pc, db, (art, loc, _, _)) in [("A", &a, ids[0]), ("B", &b, ids[1])] {
        assert!(
            over_items(db, art, loc).await.0.is_empty(),
            "{pc}: its own receipt alone"
        );
    }
    // A receives B's receipt after its own; B receives A's after its own.
    sync(&mut server, "A", &a).await;
    sync(&mut server, "B", &b).await;
    sync(&mut server, "A", &a).await;

    let mut seen = Vec::new();
    for (pc, db, (art, loc, _, _)) in [("A", &a, ids[0]), ("B", &b, ids[1])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 11_000, "{pc}: both receipts applied");
        let (open, closed) = over_items(db, art, loc).await;
        assert_eq!((open.len(), closed.len()), (1, 0), "{pc}: one over-receipt");
        let item = &open[0];
        assert_eq!(item.kind, OVER_RECEIPT, "{pc}");
        assert_eq!(item.purchase_order_number.as_deref(), Some(po_number.as_str()), "{pc}");
        assert_eq!(
            (item.expected_qty, item.actual_qty, item.excess_qty),
            (5.0, 10.0, 5.0),
            "{pc}"
        );
        assert_eq!(item.movements.len(), 2, "{pc}");
        assert!(
            item.movements
                .iter()
                .all(|m| m.movement_type == "GR_ACCEPT"
                    && m.document_ref.as_deref().is_some_and(|r| r.starts_with("GR-")))
        );
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").over_quantity_count,
            1,
            "{pc}"
        );
        seen.push(movement_ids(item));
    }
    assert_eq!(seen[0], seen[1], "the same receipt entries on both PCs");

    let manager = user_with(&b, "overreceiptmgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_b, loc_b, _, _) = ids[1];
    let resolved = negative_stock::resolve_by_count(
        &b,
        manager,
        resolution(art_b, loc_b, 6.0, "Livraison reçue deux fois, recompté"),
    )
    .await
    .expect("the count resolves the over-receipt");
    assert_eq!(resolved.variance_qty, -5.0);
    assert!(over_items(&b, art_b, loc_b).await.0.is_empty(), "B: closed");
    sync(&mut server, "B", &b).await;
    sync(&mut server, "A", &a).await;
    let (art_a, loc_a, _, _) = ids[0];
    let (open, closed) = over_items(&a, art_a, loc_a).await;
    assert!(open.is_empty(), "A: B's count held both receipts");
    assert_eq!(closed.len(), 1);
    assert_eq!(on_hand_milli(&a, art_a, loc_a).await, 6_000, "A applies the count");
}

/// One PC's normal flows never give an item: a reservation issued in parts with a return in
/// between up to its quantity, a further issue refused; a PO line received in two receipts (one
/// with a rejected part) up to its quantity, a further receipt refused. The sync centre counts
/// nothing and a count's mark names nothing.
#[tokio::test]
async fn one_pcs_normal_flows_never_give_an_item() {
    let SharedOrder { a, ids, .. } = shared_order().await;
    let (art, loc, _, _) = ids[0];
    adjust_in(&a, art, loc, 9.0).await;
    let reservation = reserve(&a, art, loc, 5.0).await;
    issue(&a, reservation, 3.0).await.expect("issue 3");
    queries::return_reserved_stock(
        &a,
        InventoryReturnInput {
            reservation_id: reservation,
            quantity: 1.0,
            notes: None,
        },
    )
    .await
    .expect("return 1");
    issue(&a, reservation, 3.0).await.expect("issue 3 more: 5 net");
    issue(&a, reservation, 0.5)
        .await
        .expect_err("beyond the reservation: refused on one PC");

    receive(&a, ids[0], 3.0, 1.0).await.expect("3 received, 1 rejected");
    receive(&a, ids[0], 2.0, 0.0).await.expect("2 more: the 5 ordered");
    receive(&a, ids[0], 0.5, 0.0)
        .await
        .expect_err("beyond the ordered quantity: refused on one PC");

    let (open, closed) = over_items(&a, art, loc).await;
    assert!(open.is_empty() && closed.is_empty(), "no over-issue, no over-receipt");
    assert_eq!(
        negative_stock::summary(&a).await.expect("summary").over_quantity_count,
        0
    );
    let (_, follows) = {
        use sea_orm::TransactionTrait;
        let tx = a.begin().await.expect("tx");
        let mark = negative_stock::count_mark(&tx, art, loc).await.expect("mark");
        tx.rollback().await.expect("rollback");
        mark
    };
    assert_eq!(follows, "[]", "the mark names nothing");
}

async fn adjust_in(db: &DatabaseConnection, article_id: i64, location_id: i64, qty: f64) {
    queries::adjust_stock(
        db,
        crate::inventory::domain::InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: qty,
            reason_code: None,
            notes: Some("stock in".into()),
            source_ref: None,
        },
    )
    .await
    .expect("stock in");
}
