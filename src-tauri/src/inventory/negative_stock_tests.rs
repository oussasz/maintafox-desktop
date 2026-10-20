//! Stock balances below zero after sync (`inventory::negative_stock`, CMMS Advisor 2026-10-20):
//! two PCs issuing the last units, the one review item per article and location, its resolution
//! by a count, its closure by a later entry, the over-reservation warning and the local refusals.

use sea_orm::DatabaseConnection;

use crate::errors::AppError;
use crate::inventory::domain::{InventoryIssueInput, InventoryReserveInput, InventoryStockAdjustInput};
use crate::inventory::negative_stock::{
    self, ResolveNegativeStockInput, CLOSURE_COUNT, CLOSURE_RECEIPT, REFUSED_NOT_OPEN, REFUSED_REASON_SHORT,
};
use crate::inventory::queries;
use crate::sync::test_support::{exec, scalar_i64, started_device_db, CreatedRow, RowFactory, TestServer};

/// An untracked, non-critical article and a location created on `a`, with `qty` in stock there.
async fn stocked_pair(a: &DatabaseConnection, factory: &mut RowFactory, qty: f64) -> (CreatedRow, CreatedRow) {
    let article = factory.create(a, "articles").await.expect("article");
    let location = factory.create(a, "stock_locations").await.expect("location");
    // The factory fills every flag; these tests are about balances, not lots or critical spares.
    exec(
        a,
        "UPDATE articles SET requires_serial_tracking = 0, requires_batch_tracking = 0,
                requires_expiration = 0, is_critical_spare = 0, is_active = 1 WHERE id = ?",
        vec![article.rowid.into()],
    )
    .await;
    exec(
        a,
        "UPDATE stock_locations SET is_active = 1 WHERE id = ?",
        vec![location.rowid.into()],
    )
    .await;
    exec(
        a,
        "UPDATE warehouses SET is_active = 1 WHERE id = (SELECT warehouse_id FROM stock_locations WHERE id = ?)",
        vec![location.rowid.into()],
    )
    .await;
    adjust(a, article.rowid, location.rowid, qty).await.expect("stock in");
    (article, location)
}

async fn adjust(db: &DatabaseConnection, article_id: i64, location_id: i64, delta: f64) -> Result<(), AppError> {
    queries::adjust_stock(
        db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: delta,
            reason_code: None,
            notes: Some("test".into()),
            source_ref: None,
        },
    )
    .await
    .map(|_| ())
}

async fn reserve(db: &DatabaseConnection, article_id: i64, location_id: i64, qty: f64) -> Result<i64, AppError> {
    queries::reserve_stock(
        db,
        InventoryReserveInput {
            article_id,
            location_id,
            quantity: qty,
            source_type: "MANUAL".into(),
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .map(|r| r.id)
}

/// Reserves and issues `qty` at the location (the issue path of the stores).
async fn issue(db: &DatabaseConnection, article_id: i64, location_id: i64, qty: f64) {
    let reservation_id = reserve(db, article_id, location_id, qty).await.expect("reserve");
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
    .expect("issue");
}

/// The local ids of an article and a location on `db`, from their sync ids.
async fn local_pair(db: &DatabaseConnection, article: &CreatedRow, location: &CreatedRow) -> (i64, i64) {
    (
        scalar_i64(
            db,
            "SELECT id FROM articles WHERE entity_sync_id = ?",
            vec![article.entity_sync_id.clone().into()],
        )
        .await,
        scalar_i64(
            db,
            "SELECT id FROM stock_locations WHERE entity_sync_id = ?",
            vec![location.entity_sync_id.clone().into()],
        )
        .await,
    )
}

async fn on_hand_milli(db: &DatabaseConnection, article_id: i64, location_id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT CAST(ROUND(on_hand_qty * 1000) AS INTEGER) FROM stock_balances WHERE article_id = ? AND location_id = ?",
        vec![article_id.into(), location_id.into()],
    )
    .await
}

/// Rows of `stock_negative_reviews` for the pair, open or closed.
async fn review_rows(db: &DatabaseConnection, article_id: i64, location_id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM stock_negative_reviews WHERE article_id = ? AND location_id = ?",
        vec![article_id.into(), location_id.into()],
    )
    .await
}

async fn sync_clean(server: &mut TestServer, device: &str, db: &DatabaseConnection) {
    let report = server.exchange(device, db).await;
    assert_eq!(report.conflicts, 0, "{device}: server conflicts");
    assert!(
        report.materialized.errors.is_empty(),
        "{device}: {:?}",
        report.materialized.errors
    );
    assert_eq!(report.materialized.conflicts, 0, "{device}: apply conflicts");
    assert_eq!(report.materialized.deferred, 0, "{device}: deferred rows");
}

/// A plant-wide role holding `permissions`, given to a new account on `db`.
async fn user_with(db: &DatabaseConnection, name: &str, permissions: &[&str]) -> i32 {
    let user = crate::auth::scope_tests::user(db, name).await;
    let role = crate::auth::scope_tests::role(db, &format!("{name}-role"), permissions).await;
    crate::auth::scope_tests::assign(db, user, role, "tenant", None).await;
    user
}

fn resolution(article_id: i64, location_id: i64, counted: f64, reason: &str) -> ResolveNegativeStockInput {
    ResolveNegativeStockInput {
        article_id,
        location_id,
        counted_qty: counted,
        variance_reason_code: "COUNT_ERROR".into(),
        reason: reason.into(),
    }
}

fn refusal_code(err: AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

/// Two PCs each issue the last 5 units of a location: on both the location ends at −5 with one
/// open review item; a manager's count of 0 on A closes it on A, and on B once the count arrives.
#[tokio::test]
async fn two_pcs_issuing_the_last_units_leave_one_item_that_a_count_closes_on_both() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    let manager = user_with(&a, "negstockmgr", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_a, loc_a) = (article.rowid, location.rowid);
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    assert_eq!(on_hand_milli(&b, art_b, loc_b).await, 5000, "B holds the 5 units");

    issue(&a, art_a, loc_a, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    for (pc, db, art, loc) in [("A", &a, art_a, loc_a), ("B", &b, art_b, loc_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, -5000, "{pc}: both issues applied");
        let list = negative_stock::list_reviews(db, true).await.expect("list");
        let items: Vec<_> = list
            .items
            .iter()
            .filter(|i| i.article_id == art && i.location_id == loc)
            .collect();
        assert_eq!(items.len(), 1, "{pc}: one review item");
        assert_eq!(items[0].status, "open", "{pc}");
        assert_eq!(items[0].on_hand_qty, -5.0, "{pc}");
        assert!(
            list.over_reservations.is_empty(),
            "{pc}: below zero is an item, not a warning"
        );
        let summary = negative_stock::summary(db).await.expect("summary");
        assert_eq!(summary.negative_count, 1, "{pc}");
        assert_eq!(summary.over_reserved_count, 0, "{pc}");
    }

    let closed = negative_stock::resolve_by_count(&a, manager, resolution(art_a, loc_a, 0.0, "Recompté : bac vide"))
        .await
        .expect("count resolves");
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.closure_kind.as_deref(), Some(CLOSURE_COUNT));
    assert_eq!(closed.closing_movement_type.as_deref(), Some("COUNT_ADJUST"));
    assert_eq!(on_hand_milli(&a, art_a, loc_a).await, 0);
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.negative_stock.counted' AND actor_id = ?",
            vec![i64::from(manager).into()],
        )
        .await,
        1,
        "the count is audited"
    );
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM inventory_count_lines cl JOIN inventory_count_sessions cs ON cs.id = cl.session_id
             WHERE cs.status = 'posted' AND cl.article_id = ? AND cl.location_id = ?
               AND cl.counted_qty = 0 AND cl.variance_qty = 5 AND cl.approval_note = 'Recompté : bac vide'",
            vec![art_a.into(), loc_a.into()],
        )
        .await,
        1,
        "posted through the count workflow"
    );

    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, art_b, loc_b).await, 0, "B applies the count");
    let on_b = negative_stock::list_reviews(&b, true).await.expect("list B");
    let item = on_b
        .items
        .iter()
        .find(|i| i.article_id == art_b && i.location_id == loc_b)
        .expect("B item");
    assert_eq!(item.status, "closed");
    assert_eq!(item.closure_kind.as_deref(), Some(CLOSURE_COUNT));
    assert_eq!(negative_stock::summary(&b).await.expect("summary").negative_count, 0);
}

/// Two received issues on the same location give one item, not one per movement.
#[tokio::test]
async fn two_received_issues_give_one_item() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;

    issue(&a, article.rowid, location.rowid, 5.0).await;
    issue(&b, art_b, loc_b, 3.0).await;
    issue(&b, art_b, loc_b, 2.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    assert_eq!(on_hand_milli(&a, article.rowid, location.rowid).await, -5000);
    assert_eq!(
        review_rows(&a, article.rowid, location.rowid).await,
        1,
        "one item from the two received issues"
    );
    assert_eq!(
        negative_stock::list_reviews(&a, false).await.expect("list").items.len(),
        1
    );
}

/// A receipt that brings the stock back to zero or more closes the item before anyone acts,
/// noted "régularisé par réception", on the PC that made it and on the one that receives it.
#[tokio::test]
async fn a_later_receipt_closes_the_item_as_regularised_by_receipt() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;

    issue(&a, article.rowid, location.rowid, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    assert_eq!(review_rows(&b, art_b, loc_b).await, 1, "B: item open");

    // B takes 8 in (stock entry): back to 3.
    adjust(&b, art_b, loc_b, 8.0)
        .await
        .expect("entry on a location below zero");
    let on_b = negative_stock::list_reviews(&b, true).await.expect("list B");
    assert_eq!(on_b.items.len(), 1);
    assert_eq!(on_b.items[0].status, "closed");
    assert_eq!(on_b.items[0].closure_kind.as_deref(), Some(CLOSURE_RECEIPT));
    assert_eq!(on_b.items[0].on_hand_qty, 3.0);

    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, article.rowid, location.rowid).await, 3000);
    let on_a = negative_stock::list_reviews(&a, true).await.expect("list A");
    assert_eq!(on_a.items.len(), 1, "A: one item, closed on arrival of the entry");
    assert_eq!(on_a.items[0].status, "closed");
    assert_eq!(on_a.items[0].closure_kind.as_deref(), Some(CLOSURE_RECEIPT));
    assert_eq!(on_a.items[0].closing_movement_type.as_deref(), Some("ADJUST_IN"));
}

/// More reserved than held while the stock is zero or more is a warning only, no item.
#[tokio::test]
async fn over_reservation_is_a_warning_only() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;

    reserve(&a, article.rowid, location.rowid, 4.0)
        .await
        .expect("reserve on A");
    reserve(&b, art_b, loc_b, 3.0).await.expect("reserve on B");
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    for (pc, db, art, loc) in [("A", &a, article.rowid, location.rowid), ("B", &b, art_b, loc_b)] {
        let list = negative_stock::list_reviews(db, true).await.expect("list");
        assert!(list.items.is_empty(), "{pc}: no review item");
        assert_eq!(review_rows(db, art, loc).await, 0, "{pc}");
        assert_eq!(list.over_reservations.len(), 1, "{pc}: one warning");
        let w = &list.over_reservations[0];
        assert_eq!(
            (w.on_hand_qty, w.reserved_qty, w.available_qty),
            (5.0, 7.0, -2.0),
            "{pc}"
        );
        let summary = negative_stock::summary(db).await.expect("summary");
        assert_eq!((summary.negative_count, summary.over_reserved_count), (0, 1), "{pc}");
    }
}

/// A location below zero refuses local reservations, issues (also of stock reserved before) and
/// exits (available-stock checks), and the count that resolves it needs `inv.manage` and a reason
/// of at least 5 characters once trimmed.
#[tokio::test]
async fn a_negative_location_refuses_local_issues_and_the_count_needs_the_right_and_a_reason() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 6.0).await;
    let (art, loc) = (article.rowid, location.rowid);
    let held = reserve(&a, art, loc, 1.0).await.expect("reservation held for a job");
    let viewer = user_with(&a, "negstockviewer", &[crate::rbac::permissions::INV_VIEW]).await;
    let manager = user_with(&a, "negstockmanager", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    issue(&a, art, loc, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art, loc).await, -4000);

    assert!(reserve(&a, art, loc, 1.0).await.is_err(), "reservation refused");
    assert!(adjust(&a, art, loc, -1.0).await.is_err(), "exit refused");
    let issued = queries::issue_reserved_stock(
        &a,
        InventoryIssueInput {
            reservation_id: held,
            quantity: 1.0,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await;
    assert!(issued.is_err(), "issue of the stock reserved before refused");
    assert_eq!(on_hand_milli(&a, art, loc).await, -4000, "nothing moved");

    let denied = negative_stock::resolve_by_count(&a, viewer, resolution(art, loc, 0.0, "Recompté"))
        .await
        .expect_err("inv.view only");
    assert!(matches!(denied, AppError::PermissionDenied(_)), "{denied:?}");
    for short in ["", "    ", "abcd", "  ab  "] {
        let err = negative_stock::resolve_by_count(&a, manager, resolution(art, loc, 0.0, short))
            .await
            .expect_err("short reason");
        assert_eq!(refusal_code(err), REFUSED_REASON_SHORT, "{short:?}");
    }
    assert_eq!(review_rows(&a, art, loc).await, 1);
    assert_eq!(on_hand_milli(&a, art, loc).await, -4000, "refusals write nothing");

    // Counted 0 while 1 is still reserved: the item closes, the reservation shows as a warning.
    let closed = negative_stock::resolve_by_count(&a, manager, resolution(art, loc, 0.0, " abcde "))
        .await
        .expect("five characters once trimmed");
    assert_eq!(closed.status, "closed");
    assert_eq!(
        (closed.on_hand_qty, closed.reserved_qty, closed.available_qty),
        (0.0, 1.0, -1.0)
    );
    let list = negative_stock::list_reviews(&a, false).await.expect("list");
    assert!(list.items.is_empty());
    assert_eq!(list.over_reservations.len(), 1, "over-reservation warning");
    let again = negative_stock::resolve_by_count(&a, manager, resolution(art, loc, 2.0, "Recompté"))
        .await
        .expect_err("nothing left to reconcile");
    assert_eq!(refusal_code(again), REFUSED_NOT_OPEN);
}
