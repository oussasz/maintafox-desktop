//! "Sur-sortie / sur-réception — à rapprocher" (`inventory::negative_stock`, AU-103, CMMS Advisor
//! decision D12 (a)): two PCs issuing the whole of one reservation, or receiving the same PO line,
//! leave it issued or received beyond its quantity. The item is listed on every PC holding the
//! movements, whatever order they arrived in, never on one PC's normal flows. It is closed by a
//! count made where every one of its rows was held (from the list, with a reason of at least 8
//! characters, or from a count session), reopened by a row arriving after that count or by its
//! reversal; an over-issue is also closed by a return of the reservation (CMMS Advisor ruling
//! 2026-10-23), and kept with the closed items.

use sea_orm::DatabaseConnection;

use super::negative_stock_tests::{
    enter_count, local_pair, on_hand_milli, post_count, post_count_approved_by, refusal_code, resolution,
    second_location, session_by_code, stocked_pair, user_with,
};
use crate::inventory::domain::{
    InventoryIssueInput, InventoryReserveInput, InventoryReturnInput, ReceiveGoodsInput, ReceivePurchaseOrderLineInput,
};
use crate::inventory::negative_stock::{
    self, OverQuantityReview, CLOSURE_COUNT, CLOSURE_RETURN, COUNT_PATH_COUNT_SESSION, COUNT_PATH_RECONCILIATION,
    OVER_ISSUE, OVER_RECEIPT, REFUSED_NOT_OPEN, REFUSED_REASON_SHORT,
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

/// Runs `sql` on `db` with change capture suppressed: the rows keep their place in the outbox and
/// are sent as they now stand (used to set the time a movement was recorded).
async fn exec_uncaptured(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    use sea_orm::TransactionTrait;
    let tx = db.begin().await.expect("tx");
    crate::sync::capture::begin_suppression(&tx).await.expect("suppress");
    exec(&tx, sql, values).await;
    crate::sync::capture::end_suppression(&tx).await.expect("unsuppress");
    tx.commit().await.expect("commit");
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

/// The ids a count of the pair entered now on `db` would name (its mark, sorted).
async fn mark_of(db: &DatabaseConnection, article_id: i64, location_id: i64) -> Vec<String> {
    use sea_orm::TransactionTrait;
    let tx = db.begin().await.expect("tx");
    let (_, follows) = negative_stock::count_mark(&tx, article_id, location_id)
        .await
        .expect("mark");
    tx.rollback().await.expect("rollback");
    let mut ids: Vec<String> = serde_json::from_str(&follows).expect("mark list");
    ids.sort();
    ids
}

/// Sync id of the count line of the session `code` (one line).
async fn count_line_sync_id(db: &DatabaseConnection, code: &str) -> String {
    crate::sync::test_support::scalar_opt_text(
        db,
        "SELECT cl.entity_sync_id FROM inventory_count_lines cl
         JOIN inventory_count_sessions cs ON cs.id = cl.session_id WHERE cs.session_code = ?",
        vec![code.into()],
    )
    .await
    .expect("count line")
}

/// The JSON list of the counts a count line follows.
async fn follows_of(db: &DatabaseConnection, line_id: i64) -> Vec<String> {
    let json = crate::sync::test_support::scalar_opt_text(
        db,
        "SELECT count_follows_json FROM inventory_count_lines WHERE id = ?",
        vec![line_id.into()],
    )
    .await
    .expect("mark");
    serde_json::from_str(&json).expect("mark list")
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
        assert_eq!(
            mark_of(db, art, loc).await,
            movement_ids(item),
            "{pc}: a count entered here names both issues"
        );
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
    let closing = closed[0].closed_by.as_ref().expect("closed by");
    let closing_session = closing.session_code.clone();
    assert_eq!(closing_session, resolved.session_code);
    assert_eq!(closing.path, COUNT_PATH_RECONCILIATION);
    assert_eq!(closing.variance_qty, 0.0, "0 on hand, 0 counted");
    assert_eq!(
        closing.approved_by, None,
        "a count from the list without variance names no approver"
    );
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

/// A PO line of 5 made on the first PC and known to every PC (`dbs`): each PC's `(article, location,
/// PO id, PO line id)`, with, when asked, a second location of the article holding 2 (`ids2`, the
/// same tuple at that location), and the PO number.
struct SharedOrder {
    dbs: Vec<DatabaseConnection>,
    server: TestServer,
    ids: Vec<(i64, i64, i64, i64)>,
    ids2: Vec<(i64, i64, i64, i64)>,
    po_number: String,
}

/// The PCs' names on the test server, in the order of `SharedOrder::dbs`.
const PC_NAMES: [&str; 4] = ["A", "B", "C", "D"];

async fn shared_order(pcs: usize, with_second_location: bool) -> SharedOrder {
    assert!(pcs <= PC_NAMES.len());
    let mut dbs = Vec::with_capacity(pcs);
    for _ in 0..pcs {
        dbs.push(started_device_db().await);
    }
    let a = &dbs[0];
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(a, &mut factory, 1.0).await;
    let location2 = if with_second_location {
        Some(second_location(a, &mut factory, article.rowid, 2.0).await)
    } else {
        None
    };
    let line = factory.create(a, "purchase_order_lines").await.expect("PO line");
    exec(
        a,
        "UPDATE purchase_order_lines SET article_id = ?, ordered_qty = 5, received_qty = 0, status = 'OPEN',
                unit_price = 10, source_reservation_id = NULL
         WHERE id = ?",
        vec![article.rowid.into(), line.rowid.into()],
    )
    .await;
    exec(
        a,
        "UPDATE purchase_orders SET status = 'APPROVED', currency_code = NULL
         WHERE id = (SELECT purchase_order_id FROM purchase_order_lines WHERE id = ?)",
        vec![line.rowid.into()],
    )
    .await;
    let po_number = crate::sync::test_support::scalar_opt_text(
        a,
        "SELECT po.po_number FROM purchase_orders po JOIN purchase_order_lines l ON l.purchase_order_id = po.id
         WHERE l.id = ?",
        vec![line.rowid.into()],
    )
    .await
    .expect("PO number");
    for (name, db) in PC_NAMES.into_iter().zip(&dbs) {
        sync(&mut server, name, db).await;
    }
    let mut ids = Vec::with_capacity(pcs);
    let mut ids2 = Vec::new();
    for db in &dbs {
        let (art, loc) = local_pair(db, &article, &location).await;
        let line_id = local_id(db, "purchase_order_lines", &line.entity_sync_id).await;
        let po_id = scalar_i64(
            db,
            "SELECT purchase_order_id FROM purchase_order_lines WHERE id = ?",
            vec![line_id.into()],
        )
        .await;
        ids.push((art, loc, po_id, line_id));
        if let Some(location2) = &location2 {
            let loc2 = local_id(db, "stock_locations", &location2.entity_sync_id).await;
            ids2.push((art, loc2, po_id, line_id));
        }
    }
    SharedOrder {
        dbs,
        server,
        ids,
        ids2,
        po_number,
    }
}

/// Sync ids of the receipt lines of a PO line at a location held on `db`.
async fn receipt_line_ids(db: &DatabaseConnection, line_id: i64, location_id: i64) -> Vec<String> {
    let rows = sea_orm::ConnectionTrait::query_all(
        db,
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "SELECT entity_sync_id FROM goods_receipt_lines WHERE po_line_id = ? AND location_id = ?
             ORDER BY entity_sync_id",
            vec![line_id.into(), location_id.into()],
        ),
    )
    .await
    .expect("receipt lines");
    rows.iter()
        .map(|r| r.try_get::<String>("", "entity_sync_id").expect("sync id"))
        .collect()
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
        dbs,
        mut server,
        ids,
        po_number,
        ..
    } = shared_order(2, false).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    receive(a, ids[0], 5.0, 0.0).await.expect("A receives the line");
    receive(b, ids[1], 5.0, 0.0).await.expect("B receives the line");
    for (pc, db, (art, loc, _, _)) in [("A", a, ids[0]), ("B", b, ids[1])] {
        assert!(
            over_items(db, art, loc).await.0.is_empty(),
            "{pc}: its own receipt alone"
        );
    }
    // A receives B's receipt after its own; B receives A's after its own.
    sync(&mut server, "A", a).await;
    sync(&mut server, "B", b).await;
    sync(&mut server, "A", a).await;

    let mut seen = Vec::new();
    for (pc, db, (art, loc, _, _)) in [("A", a, ids[0]), ("B", b, ids[1])] {
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

    let manager = user_with(b, "overreceiptmgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_b, loc_b, _, _) = ids[1];
    let resolved = negative_stock::resolve_by_count(
        b,
        manager,
        resolution(art_b, loc_b, 6.0, "Livraison reçue deux fois, recompté"),
    )
    .await
    .expect("the count resolves the over-receipt");
    assert_eq!(resolved.variance_qty, -5.0);
    assert!(over_items(b, art_b, loc_b).await.0.is_empty(), "B: closed");
    sync(&mut server, "B", b).await;
    sync(&mut server, "A", a).await;
    let (art_a, loc_a, _, _) = ids[0];
    let (open, closed) = over_items(a, art_a, loc_a).await;
    assert!(open.is_empty(), "A: B's count held both receipts");
    assert_eq!(closed.len(), 1);
    assert_eq!(on_hand_milli(a, art_a, loc_a).await, 6_000, "A applies the count");
}

/// One PC's normal flows never give an item: a reservation issued in parts with a return in
/// between up to its quantity, a further issue refused; a PO line received in two receipts (one
/// with a rejected part) up to its quantity, a further receipt refused. The sync centre counts
/// nothing and a count's mark names nothing, while it names the rows of an over-receipt once B's
/// receipt of the same line arrives.
#[tokio::test]
async fn one_pcs_normal_flows_never_give_an_item() {
    let SharedOrder {
        dbs, mut server, ids, ..
    } = shared_order(2, false).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let (art, loc, _, _) = ids[0];
    adjust_in(a, art, loc, 9.0).await;
    let reservation = reserve(a, art, loc, 5.0).await;
    issue(a, reservation, 3.0).await.expect("issue 3");
    queries::return_reserved_stock(
        a,
        InventoryReturnInput {
            reservation_id: reservation,
            quantity: 1.0,
            notes: None,
        },
    )
    .await
    .expect("return 1");
    issue(a, reservation, 3.0).await.expect("issue 3 more: 5 net");
    issue(a, reservation, 0.5)
        .await
        .expect_err("beyond the reservation: refused on one PC");
    // Times are kept to the second: put the issue, the return and the issue in the same one, as a
    // quick storekeeper would, so the order never depends on the random sync ids (a return is read
    // before an issue of the same second).
    exec_uncaptured(
        a,
        "UPDATE inventory_transactions SET performed_at = '2026-10-23T08:00:00Z' WHERE reservation_id = ?",
        vec![reservation.into()],
    )
    .await;

    receive(a, ids[0], 3.0, 1.0).await.expect("3 received, 1 rejected");
    receive(a, ids[0], 2.0, 0.0).await.expect("2 more: the 5 ordered");
    receive(a, ids[0], 0.5, 0.0)
        .await
        .expect_err("beyond the ordered quantity: refused on one PC");

    let (open, closed) = over_items(a, art, loc).await;
    assert!(open.is_empty() && closed.is_empty(), "no over-issue, no over-receipt");
    assert_eq!(
        negative_stock::summary(a).await.expect("summary").over_quantity_count,
        0
    );
    assert!(mark_of(a, art, loc).await.is_empty(), "a count's mark names nothing");

    // The same mark names exactly the rows of an item once there is one: B, not knowing A's
    // receipts, receives the whole line too.
    receive(b, ids[1], 5.0, 0.0).await.expect("B receives the line");
    sync(&mut server, "B", b).await;
    sync(&mut server, "A", a).await;
    let (open, _) = over_items(a, art, loc).await;
    assert_eq!(open.len(), 1, "3 + 2 + 5 received for 5 ordered");
    assert_eq!(open[0].kind, OVER_RECEIPT);
    let mut rows = receipt_line_ids(a, ids[0].3, loc).await;
    assert_eq!(rows.len(), 3);
    rows.extend(movement_ids(&open[0]));
    rows.sort();
    assert_eq!(
        mark_of(a, art, loc).await,
        rows,
        "its receipt lines and stock entries, nothing of the reservation"
    );
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

/// A return of the reservation (CMMS Advisor ruling 2026-10-23, Q1 (a)): after A and B each issued
/// the 5 reserved, the manager's count on A closes the over-issue; the storekeeper then returns 5
/// against the reservation on A. The item is closed "régularisé par retour" with that return (who,
/// when, 5), listed with the closed items on every PC, not to reconcile any more; a count entered
/// later on B, from a count session, still follows the manager's count although the item is no
/// longer the one that count closed.
#[tokio::test]
async fn a_return_of_the_reservation_closes_the_over_issue_and_keeps_it_listed() {
    let SharedReservation {
        dbs: [a, b, c],
        mut server,
        ids,
    } = shared_reservation().await;
    let manager = user_with(&a, "overissuereturnmgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    let storekeeper = user_with(&a, "overissuereturner", &[crate::rbac::permissions::INV_MANAGE]).await;
    issue(&a, ids[0].2, 5.0).await.expect("A issues");
    issue(&b, ids[1].2, 5.0).await.expect("B issues");
    for (name, db) in [("B", &b), ("A", &a)] {
        sync(&mut server, name, db).await;
    }
    let (art_a, loc_a, reservation_a) = ids[0];
    let counted = negative_stock::resolve_by_count(
        &a,
        manager,
        resolution(art_a, loc_a, 0.0, "Bac recompté : 5 sortis deux fois"),
    )
    .await
    .expect("the count closes the over-issue");
    let (open, closed) = over_items(&a, art_a, loc_a).await;
    assert!(open.is_empty());
    assert_eq!(closed[0].closure_kind.as_deref(), Some(CLOSURE_COUNT));

    queries::return_reserved_stock_as(
        &a,
        InventoryReturnInput {
            reservation_id: reservation_a,
            quantity: 5.0,
            notes: Some("Pièces non montées".into()),
        },
        Some(i64::from(storekeeper)),
    )
    .await
    .expect("5 returned against the reservation");
    // Times are kept to the second and a return is read before an issue of the same second: date
    // the return clearly after the issues, so a fast run never reads it as made with them.
    exec_uncaptured(
        &a,
        "UPDATE inventory_transactions SET performed_at = '2099-01-01T00:00:00Z'
         WHERE reservation_id = ? AND movement_type = 'RETURN'",
        vec![reservation_a.into()],
    )
    .await;
    for (name, db) in [("A", &a), ("B", &b), ("C", &c)] {
        sync(&mut server, name, db).await;
    }
    // A count entered on B follows the manager's: what that count knew (both issues) is held
    // on B, whatever those issues make now.
    let counter = user_with(&b, "overissuelatercount", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_b, loc_b, _) = ids[1];
    let (_, line) = enter_count(&b, counter, art_b, loc_b, 5.0).await;
    let manager_count = count_line_sync_id(&b, &counted.session_code).await;
    assert!(
        follows_of(&b, line.id).await.contains(&manager_count),
        "a later count follows the count made before the return"
    );

    let mut seen = Vec::new();
    for (pc, db, (art, loc, _)) in [("A", &a, ids[0]), ("B", &b, ids[1]), ("C", &c, ids[2])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 5_000, "{pc}: 10 − 5 − 5 + 5");
        let (open, closed) = over_items(db, art, loc).await;
        assert!(open.is_empty(), "{pc}: nothing to reconcile");
        assert_eq!(closed.len(), 1, "{pc}: kept with the closed items");
        let item = &closed[0];
        assert_eq!(item.kind, OVER_ISSUE, "{pc}");
        assert_eq!(
            item.closure_kind.as_deref(),
            Some(CLOSURE_RETURN),
            "{pc}: régularisé par retour"
        );
        assert_eq!(item.closed_by, None, "{pc}: the count did not know the return");
        let by = item.closed_by_return.as_ref().expect("the return");
        assert_eq!(by.movement_type, "RETURN", "{pc}");
        assert_eq!(by.quantity, 5.0, "{pc}");
        assert_eq!(by.posted_by.as_deref(), Some("overissuereturner"), "{pc}");
        assert!(!by.posted_at.is_empty(), "{pc}");
        assert_eq!(
            (item.expected_qty, item.actual_qty, item.excess_qty),
            (5.0, 10.0, 5.0),
            "{pc}: what was issued beyond the reservation before the return"
        );
        assert_eq!(item.movements.len(), 3, "{pc}: both issues and the return");
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").over_quantity_count,
            0,
            "{pc}"
        );
        seen.push((movement_ids(item), by.movement_sync_id.clone()));
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "the same on every PC: {seen:?}");
    let again = negative_stock::resolve_by_count(&a, manager, resolution(art_a, loc_a, 5.0, "Recompté après retour"))
        .await
        .expect_err("nothing left to reconcile");
    assert_eq!(refusal_code(again), REFUSED_NOT_OPEN);
}

/// Parts brought back without naming the reservation (a stock entry) are not a return of it: the
/// over-issue stays to reconcile. A zero-variance count of a count session on A, which holds both
/// issues, closes it (CMMS Advisor ruling Q2 (a)) — no reason of 8 characters is asked there — and
/// the closed item names that session, path and who counted; a count without variance has no
/// approver.
#[tokio::test]
async fn a_return_naming_no_reservation_leaves_the_over_issue_to_a_count() {
    let SharedReservation {
        dbs: [a, b, _c],
        mut server,
        ids,
    } = shared_reservation().await;
    issue(&a, ids[0].2, 5.0).await.expect("A issues");
    issue(&b, ids[1].2, 5.0).await.expect("B issues");
    for (name, db) in [("B", &b), ("A", &a)] {
        sync(&mut server, name, db).await;
    }
    let (art, loc, _) = ids[0];
    adjust_in(&a, art, loc, 5.0).await;
    let (open, closed) = over_items(&a, art, loc).await;
    assert_eq!((open.len(), closed.len()), (1, 0), "still to reconcile");
    assert_eq!(open[0].actual_qty, 10.0);
    assert_eq!(open[0].closure_kind, None);

    let counter = user_with(&a, "overissuecyclecount", &[crate::rbac::permissions::INV_MANAGE]).await;
    let session = post_count(&a, counter, enter_count(&a, counter, art, loc, 5.0).await).await;
    let (open, closed) = over_items(&a, art, loc).await;
    assert!(open.is_empty(), "the cycle count held both issues");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].closure_kind.as_deref(), Some(CLOSURE_COUNT));
    let by = closed[0].closed_by.as_ref().expect("closed by the count");
    assert_eq!(by.session_code, session.session_code);
    assert_eq!(by.path, COUNT_PATH_COUNT_SESSION);
    assert_eq!(by.counted_by.as_deref(), Some("overissuecyclecount"));
    assert_eq!(by.variance_qty, 0.0);
    assert_eq!(by.approved_by, None, "no variance, no approval");
}

/// A PO line received on A into the first location and on B into a second one: one over-receipt
/// per location, each closed only by a count of its own location — the first by the manager's
/// count from the list on A, the second by a count session on B whose variance another user
/// approved; the closed item names that session, who counted and who approved, on both PCs.
#[tokio::test]
async fn an_over_receipt_into_two_locations_is_closed_by_each_locations_own_count() {
    let SharedOrder {
        dbs,
        mut server,
        ids,
        ids2,
        ..
    } = shared_order(2, true).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    receive(a, ids[0], 5.0, 0.0)
        .await
        .expect("A receives into the first location");
    receive(b, ids2[1], 5.0, 0.0)
        .await
        .expect("B receives into the second location");
    for (name, db) in [("A", a), ("B", b), ("A", a)] {
        sync(&mut server, name, db).await;
    }
    for (pc, db, first, second) in [("A", a, ids[0], ids2[0]), ("B", b, ids[1], ids2[1])] {
        for (place, (art, loc, _, _)) in [("first", first), ("second", second)] {
            let (open, closed) = over_items(db, art, loc).await;
            assert_eq!((open.len(), closed.len()), (1, 0), "{pc}: {place} location");
            assert_eq!(
                (open[0].actual_qty, open[0].excess_qty, open[0].movements.len()),
                (10.0, 5.0, 1),
                "{pc}: {place} location lists its own entry"
            );
        }
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").over_quantity_count,
            2,
            "{pc}"
        );
    }

    let manager = user_with(a, "overrcptlocmgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_a, loc_a, _, _) = ids[0];
    // 1 + 5 on hand, 1 missing: a variance, which the person counting approves.
    negative_stock::resolve_by_count(a, manager, resolution(art_a, loc_a, 5.0, "Premier bac recompté"))
        .await
        .expect("the first location's count");
    sync(&mut server, "A", a).await;
    sync(&mut server, "B", b).await;
    for (pc, db, first, second) in [("A", a, ids[0], ids2[0]), ("B", b, ids[1], ids2[1])] {
        let (open, closed) = over_items(db, first.0, first.1).await;
        assert_eq!((open.len(), closed.len()), (0, 1), "{pc}: the first location is closed");
        let by = closed[0].closed_by.as_ref().expect("its count");
        assert_eq!(by.path, COUNT_PATH_RECONCILIATION, "{pc}");
        assert_eq!(by.variance_qty, -1.0, "{pc}");
        assert_eq!(
            by.approved_by.as_deref(),
            Some("overrcptlocmgr"),
            "{pc}: a variance counted from the list is approved by the person counting"
        );
        let (open, closed) = over_items(db, second.0, second.1).await;
        assert_eq!((open.len(), closed.len()), (1, 0), "{pc}: not the second one");
    }

    let counter = user_with(b, "overrcptloccounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let approver = user_with(b, "overrcptlocapprover", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_b, loc2_b, _, _) = ids2[1];
    let session = post_count_approved_by(b, counter, approver, enter_count(b, counter, art_b, loc2_b, 2.0).await).await;
    sync(&mut server, "B", b).await;
    sync(&mut server, "A", a).await;
    for (pc, db, (art, loc2, _, _)) in [("A", a, ids2[0]), ("B", b, ids2[1])] {
        assert_eq!(on_hand_milli(db, art, loc2).await, 2_000, "{pc}: 2 + 5 counted at 2");
        let (open, closed) = over_items(db, art, loc2).await;
        assert_eq!(
            (open.len(), closed.len()),
            (0, 1),
            "{pc}: the second location is closed"
        );
        let by = closed[0].closed_by.as_ref().expect("its count session");
        assert_eq!(by.session_code, session.session_code, "{pc}");
        assert_eq!(by.path, COUNT_PATH_COUNT_SESSION, "{pc}");
        assert_eq!(by.counted_by.as_deref(), Some("overrcptloccounter"), "{pc}");
        assert_eq!(by.approved_by.as_deref(), Some("overrcptlocapprover"), "{pc}");
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").over_quantity_count,
            0,
            "{pc}"
        );
    }
}

/// An issue that arrives after the closing count reopens the over-issue: A, B and C each issue
/// the 5 reserved; A counts holding A's and B's issues, which closes it on A; C's issue arrives
/// afterwards, which the count did not know: it is to reconcile again (issued 15), on A and C.
#[tokio::test]
async fn an_issue_arriving_after_the_closing_count_reopens_the_over_issue() {
    let SharedReservation {
        dbs: [a, b, c],
        mut server,
        ids,
    } = shared_reservation().await;
    let manager = user_with(&a, "overissuereopen", &[crate::rbac::permissions::INV_MANAGE]).await;
    for (db, (_, _, reservation)) in [(&a, ids[0]), (&b, ids[1]), (&c, ids[2])] {
        issue(db, reservation, 5.0)
            .await
            .expect("each PC issues the reservation");
    }
    for (name, db) in [("B", &b), ("A", &a)] {
        sync(&mut server, name, db).await;
    }
    let (art_a, loc_a, _) = ids[0];
    negative_stock::resolve_by_count(&a, manager, resolution(art_a, loc_a, 0.0, "Recompté sur le poste A"))
        .await
        .expect("closed by the count");
    assert!(over_items(&a, art_a, loc_a).await.0.is_empty());

    for (name, db) in [("A", &a), ("C", &c), ("A", &a)] {
        sync(&mut server, name, db).await;
    }
    for (pc, db, (art, loc, _)) in [("A", &a, ids[0]), ("C", &c, ids[2])] {
        let (open, closed) = over_items(db, art, loc).await;
        assert_eq!((open.len(), closed.len()), (1, 0), "{pc}: reopened by C's issue");
        assert_eq!(
            (open[0].actual_qty, open[0].excess_qty, open[0].movements.len()),
            (15.0, 10.0, 3),
            "{pc}"
        );
        assert_eq!(open[0].closed_by, None, "{pc}");
    }
}

/// A receipt line held without its stock entry, and a stock entry held without its receipt line:
/// A and B each receive the whole line; C does not get B's stock entry yet, D not B's receipt line.
/// C lists the over-receipt with A's entry only (the receipt lines say 10 received); D lists none
/// (an entry is summed only with its receipt line). C's count, made then, does not hold B's entry:
/// once the held rows arrive, every PC lists the same open item with both entries.
#[tokio::test]
async fn a_receipt_line_and_its_stock_entry_arriving_apart_give_the_same_item_everywhere() {
    let SharedOrder {
        dbs, mut server, ids, ..
    } = shared_order(4, false).await;
    receive(&dbs[0], ids[0], 5.0, 0.0).await.expect("A receives the line");
    receive(&dbs[1], ids[1], 5.0, 0.0).await.expect("B receives the line");
    let b_line = crate::sync::test_support::scalar_opt_text(
        &dbs[1],
        "SELECT entity_sync_id FROM goods_receipt_lines WHERE po_line_id = ?",
        vec![ids[1].3.into()],
    )
    .await
    .expect("B's receipt line");
    let b_entry = crate::sync::test_support::scalar_opt_text(
        &dbs[1],
        "SELECT entity_sync_id FROM inventory_transactions WHERE movement_type = 'GR_ACCEPT'",
        vec![],
    )
    .await
    .expect("B's stock entry");
    sync(&mut server, "A", &dbs[0]).await;
    sync(&mut server, "B", &dbs[1]).await;
    server.hold_back("C", &b_entry);
    server.hold_back("D", &b_line);
    sync(&mut server, "C", &dbs[2]).await;
    sync(&mut server, "D", &dbs[3]).await;

    let (art_c, loc_c, _, _) = ids[2];
    let (open, _) = over_items(&dbs[2], art_c, loc_c).await;
    assert_eq!(open.len(), 1, "C: the receipt lines say 10 received");
    assert_eq!((open[0].actual_qty, open[0].movements.len()), (10.0, 1), "C");
    assert!(open[0].movements.iter().all(|m| m.movement_sync_id != b_entry), "C");
    let (art_d, loc_d, _, _) = ids[3];
    assert!(
        over_items(&dbs[3], art_d, loc_d).await.0.is_empty(),
        "D: B's entry without its line is not summed"
    );
    assert_eq!(
        negative_stock::summary(&dbs[3])
            .await
            .expect("summary")
            .over_quantity_count,
        0
    );

    let manager = user_with(&dbs[2], "overrcptheld", &[crate::rbac::permissions::INV_MANAGE]).await;
    negative_stock::resolve_by_count(
        &dbs[2],
        manager,
        resolution(art_c, loc_c, 11.0, "Recompté sur le poste C"),
    )
    .await
    .expect("C's count");
    assert!(
        over_items(&dbs[2], art_c, loc_c).await.0.is_empty(),
        "C: closed by its count"
    );

    server.release("C");
    server.release("D");
    for name in ["C", "D", "A", "B", "C", "D"] {
        let i = PC_NAMES.iter().position(|n| *n == name).expect("PC");
        sync(&mut server, name, &dbs[i]).await;
    }
    let mut seen = Vec::new();
    for (i, (art, loc, _, _)) in ids.iter().copied().enumerate() {
        let pc = PC_NAMES[i];
        let (open, closed) = over_items(&dbs[i], art, loc).await;
        assert_eq!(
            (open.len(), closed.len()),
            (1, 0),
            "{pc}: C's count did not hold B's entry"
        );
        assert_eq!((open[0].actual_qty, open[0].movements.len()), (10.0, 2), "{pc}");
        seen.push(movement_ids(&open[0]));
    }
    assert!(
        seen.windows(2).all(|w| w[0] == w[1]),
        "the same entries everywhere: {seen:?}"
    );
}

/// Reversing the count that closed an over-receipt reopens it, on the PC that reversed it and on
/// the others.
#[tokio::test]
async fn reversing_the_closing_count_reopens_the_over_receipt() {
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let SharedOrder {
        dbs, mut server, ids, ..
    } = shared_order(2, false).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    receive(a, ids[0], 5.0, 0.0).await.expect("A receives");
    receive(b, ids[1], 5.0, 0.0).await.expect("B receives");
    for (name, db) in [("A", a), ("B", b), ("A", a)] {
        sync(&mut server, name, db).await;
    }
    let manager = user_with(b, "overrcptreverse", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_b, loc_b, _, _) = ids[1];
    let resolved =
        negative_stock::resolve_by_count(b, manager, resolution(art_b, loc_b, 6.0, "Livraison reçue deux fois"))
            .await
            .expect("closed by the count");
    assert!(over_items(b, art_b, loc_b).await.0.is_empty());
    let session = session_by_code(b, &resolved.session_code).await;
    crate::inventory::controls::reverse_count_session(
        b,
        ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Compté sur le mauvais bac".into(),
            actor_id: Some(i64::from(manager)),
        },
    )
    .await
    .expect("reversal");
    sync(&mut server, "B", b).await;
    sync(&mut server, "A", a).await;
    for (pc, db, (art, loc, _, _)) in [("A", a, ids[0]), ("B", b, ids[1])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 11_000, "{pc}: the count undone");
        let (open, closed) = over_items(db, art, loc).await;
        assert_eq!((open.len(), closed.len()), (1, 0), "{pc}: to reconcile again");
    }
}

/// An issue or return of `quantity` recorded at `t` (seconds), as the walk reads it.
fn walk_movement_of(id: &str, movement_type: &str, t: u32, quantity: f64) -> negative_stock::OverQuantityMovement {
    negative_stock::OverQuantityMovement {
        movement_sync_id: id.to_string(),
        movement_type: movement_type.to_string(),
        document_ref: None,
        posted_by: None,
        posted_at: format!("2026-10-23T00:{:02}:{:02}Z", t / 60, t % 60),
        quantity,
    }
}

/// An issue or return of 5 recorded at `t` (seconds).
fn walk_movement(id: &str, movement_type: &str, t: u32) -> negative_stock::OverQuantityMovement {
    walk_movement_of(id, movement_type, t, 5.0)
}

/// The walk over `movements`, given in any order (the walk takes the order every PC shares), for a
/// reservation of 5: `None`, or `(actual, sync id of the closing return)`.
fn walk(movements: Vec<negative_stock::OverQuantityMovement>) -> Option<(f64, Option<String>)> {
    negative_stock::over_issue_walk(&movements, 5.0)
        .map(|w| (w.actual_qty, w.closing_return.map(|r| r.movement_sync_id)))
}

/// The walk with interleaved times (reservation of 5, every movement 5). Open depends on the final
/// net only; the clock decides only whether a closed-by-return entry is shown.
#[test]
fn the_over_issue_walk_follows_the_recorded_times() {
    // Two issues, then the return: over, then closed by that return.
    assert_eq!(
        walk(vec![
            walk_movement("i1", "ISSUE", 10),
            walk_movement("i2", "ISSUE", 20),
            walk_movement("r", "RETURN", 30),
        ]),
        Some((10.0, Some("r".into()))),
        "closed by the return, at its highest (10) before it"
    );
    // A false closed entry: A issues at t10; B, its clock ahead, returns at "t30"; A, having
    // received the return, issues again at t20. The net was never above 5, but the walk reads
    // +5, +5, −5: closed by return, never open.
    assert_eq!(
        walk(vec![
            walk_movement("a1", "ISSUE", 10),
            walk_movement("b-return", "RETURN", 30),
            walk_movement("a2", "ISSUE", 20),
        ]),
        Some((10.0, Some("b-return".into()))),
        "a skewed clock shows a closed-by-return entry that did not happen, never an open item"
    );
    // A hidden entry: A issues with its clock ahead ("t100"); B issues at t50 and returns at t60.
    // The net really went to 10, then back to 5; the walk reads +5, −5, +5 and finds nothing.
    assert_eq!(
        walk(vec![
            walk_movement("a1", "ISSUE", 100),
            walk_movement("b1", "ISSUE", 50),
            walk_movement("b-return", "RETURN", 60),
        ]),
        None,
        "a skewed clock hides a closed-by-return entry, never an open item"
    );
    // A return before the second issue: the reservation was never above 5.
    assert_eq!(
        walk(vec![
            walk_movement("i1", "ISSUE", 10),
            walk_movement("r", "RETURN", 20),
            walk_movement("i2", "ISSUE", 30),
        ]),
        None,
        "issue, return, issue: never over"
    );
    // Closed by a return, then issued beyond again: open, and no closing return.
    assert_eq!(
        walk(vec![
            walk_movement("i1", "ISSUE", 10),
            walk_movement("i2", "ISSUE", 20),
            walk_movement("r", "RETURN", 30),
            walk_movement("i3", "ISSUE", 40),
        ]),
        Some((10.0, None)),
        "a later issue reopens it"
    );
    // Whatever the order, a final net above the reservation is open.
    for order in [[10, 20, 30], [30, 10, 20], [20, 30, 10]] {
        assert_eq!(
            walk(vec![
                walk_movement("i1", "ISSUE", order[0]),
                walk_movement("i2", "ISSUE", order[1]),
                walk_movement("i3", "ISSUE", order[2]),
            ]),
            Some((15.0, None)),
            "{order:?}"
        );
    }
    // Same second (Stock Lead ruling: a return before an issue of the same second). One PC issues
    // 3, returns 1 and issues 3 within one second: no entry, whatever the sync ids.
    let ids = ["a", "b", "c"];
    for (i, j, k) in [(0, 1, 2), (0, 2, 1), (1, 0, 2), (1, 2, 0), (2, 0, 1), (2, 1, 0)] {
        assert_eq!(
            walk(vec![
                walk_movement_of(ids[i], "ISSUE", 10, 3.0),
                walk_movement_of(ids[j], "RETURN", 10, 1.0),
                walk_movement_of(ids[k], "ISSUE", 10, 3.0),
            ]),
            None,
            "one PC's issue, return, issue in one second, sync ids {:?}",
            (ids[i], ids[j], ids[k])
        );
    }
    // Two PCs each issue the 5 reserved and B returns 5, all in one second: really over, then
    // back. The return is read first (−5, +5, +5): never over, so the history is hidden (no item,
    // no count closure shown). Accepted by the ruling; the status is right (nothing to reconcile).
    assert_eq!(
        walk(vec![
            walk_movement("a-issue", "ISSUE", 10),
            walk_movement("b-issue", "ISSUE", 10),
            walk_movement("b-return", "RETURN", 10),
        ]),
        None,
        "two PCs' over-issue and the return in one second: hidden history"
    );
    // The same second never hides an open item: two issues and no return.
    assert_eq!(
        walk(vec![
            walk_movement("a-issue", "ISSUE", 10),
            walk_movement("b-issue", "ISSUE", 10),
        ]),
        Some((10.0, None))
    );
}

/// A count stays followed once the rows it knew make no item any more (`held_rows_named`): A and B
/// each issue the 5 reserved, the manager's count on A (no variance) closes the over-issue, then 5
/// are returned against the reservation on A. On B the return is recorded between the two issues
/// (a skewed clock): the walk never goes beyond 5 and B lists no item, so the issues are not among
/// the rows of an item. A count entered on B still follows the manager's count, since both issues
/// it knew are held on B.
#[tokio::test]
async fn a_count_stays_followed_when_its_rows_no_longer_make_an_item() {
    let SharedReservation {
        dbs: [a, b, _c],
        mut server,
        ids,
    } = shared_reservation().await;
    let manager = user_with(&a, "overissueheldmgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    issue(&a, ids[0].2, 5.0).await.expect("A issues");
    issue(&b, ids[1].2, 5.0).await.expect("B issues");
    for (name, db) in [("B", &b), ("A", &a)] {
        sync(&mut server, name, db).await;
    }
    let (art_a, loc_a, reservation_a) = ids[0];
    let counted = negative_stock::resolve_by_count(
        &a,
        manager,
        resolution(art_a, loc_a, 0.0, "Bac recompté : 5 sortis deux fois"),
    )
    .await
    .expect("the count closes the over-issue");
    queries::return_reserved_stock(
        &a,
        InventoryReturnInput {
            reservation_id: reservation_a,
            quantity: 5.0,
            notes: None,
        },
    )
    .await
    .expect("5 returned against the reservation");
    sync(&mut server, "A", &a).await;
    sync(&mut server, "B", &b).await;

    let (art_b, loc_b, reservation_b) = ids[1];
    for (sql, at) in [
        (
            "UPDATE inventory_transactions SET performed_at = ? WHERE id = (SELECT MIN(id) FROM inventory_transactions
              WHERE reservation_id = ? AND movement_type = 'ISSUE')",
            "2026-10-23T08:00:10Z",
        ),
        (
            "UPDATE inventory_transactions SET performed_at = ? WHERE reservation_id = ? AND movement_type = 'RETURN'",
            "2026-10-23T08:00:20Z",
        ),
        (
            "UPDATE inventory_transactions SET performed_at = ? WHERE id = (SELECT MAX(id) FROM inventory_transactions
              WHERE reservation_id = ? AND movement_type = 'ISSUE')",
            "2026-10-23T08:00:30Z",
        ),
    ] {
        exec(&b, sql, vec![at.into(), reservation_b.into()]).await;
    }
    let (open, closed) = over_items(&b, art_b, loc_b).await;
    assert!(
        open.is_empty() && closed.is_empty(),
        "B: issue, return, issue never goes beyond 5"
    );

    let counter = user_with(&b, "overissueheldcount", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, line) = enter_count(&b, counter, art_b, loc_b, 5.0).await;
    let manager_count = count_line_sync_id(&b, &counted.session_code).await;
    assert!(
        follows_of(&b, line.id).await.contains(&manager_count),
        "the count entered on B follows the manager's: both issues it knew are held on B"
    );
}
