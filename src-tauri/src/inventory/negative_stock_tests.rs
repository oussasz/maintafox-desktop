//! Stock to reconcile after sync (`inventory::negative_stock`, CMMS Advisor 2026-10-20 and ruling
//! S1): two PCs issuing the last units, the one review item per article and location whatever the
//! arrival order, its notice, its resolution by a count, its closure by a transfer in or a manual
//! entry, the over-reservation warning, the double count of two PCs resolving offline, and the
//! local refusals.

use sea_orm::DatabaseConnection;

use crate::errors::AppError;
use crate::inventory::domain::{InventoryIssueInput, InventoryReserveInput, InventoryStockAdjustInput};
use crate::inventory::negative_stock::{
    self, ResolveNegativeStockInput, CLOSURE_ADJUSTMENT, CLOSURE_COUNT, CLOSURE_RECEIPT, CLOSURE_RETURN,
    REFUSED_ENTRY_REASON_SHORT, REFUSED_NOT_OPEN, REFUSED_REASON_SHORT,
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
    adjust_noted(db, article_id, location_id, delta, "test").await
}

/// A manual movement with its comment (the reason of an entry at a location below zero).
async fn adjust_noted(
    db: &DatabaseConnection,
    article_id: i64,
    location_id: i64,
    delta: f64,
    notes: &str,
) -> Result<(), AppError> {
    queries::adjust_stock(
        db,
        InventoryStockAdjustInput {
            article_id,
            location_id,
            delta_qty: delta,
            reason_code: None,
            notes: Some(notes.into()),
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

/// A second location of the same article with `qty` in stock, created on `a`.
async fn second_location(a: &DatabaseConnection, factory: &mut RowFactory, article_id: i64, qty: f64) -> CreatedRow {
    let location = factory.create(a, "stock_locations").await.expect("location");
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
    adjust(a, article_id, location.rowid, qty).await.expect("stock in");
    location
}

/// One round of the stock-to-reconcile watcher, as the notification scheduler runs it.
async fn watch(db: &DatabaseConnection) -> crate::notifications::watchers::WatchReport {
    use sea_orm::TransactionTrait;
    let tx = db.begin().await.expect("tx");
    crate::sync::capture::begin_suppression(&tx).await.expect("suppress");
    let report = crate::notifications::watchers::run_watcher(
        &tx,
        negative_stock::NOTIFICATION_CATEGORY,
        chrono::Utc::now(),
        false,
    )
    .await
    .expect("watcher");
    crate::sync::capture::end_suppression(&tx).await.expect("unsuppress");
    tx.commit().await.expect("commit");
    report
}

/// `(events, open events, ids joined)` of the stock-to-reconcile notices on `db`.
async fn notices(db: &DatabaseConnection) -> (i64, i64, String) {
    (
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM notification_events WHERE category_code = ?",
            vec![negative_stock::NOTIFICATION_CATEGORY.into()],
        )
        .await,
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM notification_events WHERE category_code = ? AND resolved_at IS NULL",
            vec![negative_stock::NOTIFICATION_CATEGORY.into()],
        )
        .await,
        crate::sync::test_support::scalar_opt_text(
            db,
            "SELECT group_concat(entity_sync_id, ',') FROM (SELECT entity_sync_id FROM notification_events
             WHERE category_code = ? ORDER BY entity_sync_id)",
            vec![negative_stock::NOTIFICATION_CATEGORY.into()],
        )
        .await
        .unwrap_or_default(),
    )
}

async fn notified(db: &DatabaseConnection, user: i32) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id
         WHERE e.category_code = ? AND n.recipient_user_id = ?",
        vec![negative_stock::NOTIFICATION_CATEGORY.into(), i64::from(user).into()],
    )
    .await
}

/// Two PCs each issue the last 5 units of a location: on both the location ends at −5 with one
/// open review item and one notice to the inv.manage holders (the same on both, never repeated);
/// a manager's count of 0 on A closes it on A, and on B once the count arrives.
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
    // Both PCs watch before anything happens: the notice is new when it comes.
    watch(&a).await;
    watch(&b).await;

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
        assert!(list.double_counts.is_empty(), "{pc}");
        let summary = negative_stock::summary(db).await.expect("summary");
        assert_eq!(summary.negative_count, 1, "{pc}");
        assert_eq!(summary.over_reserved_count, 0, "{pc}");
        let first = watch(db).await;
        assert_eq!(first.raised, 1, "{pc}: one notice");
        let again = watch(db).await;
        assert_eq!(
            (again.raised, again.already_true),
            (0, 0),
            "{pc}: never repeated while open"
        );
    }
    let (on_a, on_b) = (notices(&a).await, notices(&b).await);
    assert_eq!((on_a.0, on_a.1), (1, 1));
    assert_eq!(on_a.2, on_b.2, "the same notice on both PCs");
    assert_eq!(notified(&a, manager).await, 1, "the inv.manage holder is told");

    let resolved = negative_stock::resolve_by_count(&a, manager, resolution(art_a, loc_a, 0.0, "Recompté : bac vide"))
        .await
        .expect("count resolves");
    let closed = resolved.review.expect("the negative item");
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.closure_kind.as_deref(), Some(CLOSURE_COUNT));
    assert_eq!(closed.closing_movement_type.as_deref(), Some("COUNT_ADJUST"));
    assert_eq!(resolved.variance_qty, 5.0);
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
               AND cl.counted_qty = 0 AND cl.variance_qty = 5 AND cl.approval_note = 'Recompté : bac vide'
               AND cl.count_follows_json = '[]' AND cl.count_device_code IS NOT NULL
               AND cl.count_path = 'reconciliation'",
            vec![art_a.into(), loc_a.into()],
        )
        .await,
        1,
        "posted through the count workflow, marked as a reconciliation count"
    );
    assert_eq!(watch(&a).await.resolved, 1, "the notice is resolved with the item");

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
    assert!(on_b.double_counts.is_empty(), "one count is not a double count");
    assert_eq!(negative_stock::summary(&b).await.expect("summary").negative_count, 0);
    assert_eq!(watch(&b).await.resolved, 1);
}

/// The two issues reach a third PC in the opposite order from the first PC: the same balance, one
/// item and the same notice everywhere.
#[tokio::test]
async fn the_arrival_order_does_not_change_the_item_or_its_notice() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let c = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "C", &c).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art_c, loc_c) = local_pair(&c, &article, &location).await;

    // C receives B's issue first, then A's; A receives its own first, then B's.
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "C", &c).await;
    assert_eq!(on_hand_milli(&c, art_c, loc_c).await, 0, "C: B's issue only");
    assert_eq!(review_rows(&c, art_c, loc_c).await, 0);
    issue(&a, article.rowid, location.rowid, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "C", &c).await;
    sync_clean(&mut server, "B", &b).await;

    let mut ids = Vec::new();
    for (pc, db, art, loc) in [
        ("A", &a, article.rowid, location.rowid),
        ("B", &b, art_b, loc_b),
        ("C", &c, art_c, loc_c),
    ] {
        assert_eq!(on_hand_milli(db, art, loc).await, -5000, "{pc}");
        let list = negative_stock::list_reviews(db, true).await.expect("list");
        assert_eq!(list.items.len(), 1, "{pc}: one item");
        assert_eq!(list.items[0].status, "open", "{pc}");
        watch(db).await;
        let (events, open, id) = notices(db).await;
        assert_eq!((events, open), (1, 1), "{pc}: one notice");
        ids.push(id);
    }
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "the same notice on every PC: {ids:?}"
    );
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

/// A transfer in that brings the stock back to zero or more closes the item before anyone acts,
/// noted "régularisé par réception", on the PC that made it and on the one that receives it.
#[tokio::test]
async fn a_transfer_in_closes_the_item_as_regularised_by_receipt() {
    use crate::inventory::domain::InventoryTransferInput;
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    let spare = second_location(&a, &mut factory, article.rowid, 10.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (_, spare_b) = local_pair(&b, &article, &spare).await;

    issue(&a, article.rowid, location.rowid, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    assert_eq!(review_rows(&b, art_b, loc_b).await, 1, "B: item open");

    let transfer = |quantity: f64| InventoryTransferInput {
        article_id: art_b,
        from_location_id: spare_b,
        to_location_id: loc_b,
        quantity,
        source_type: None,
        source_id: None,
        source_ref: None,
        notes: Some("Réapprovisionnement depuis la réserve".into()),
        lot_id: None,
    };
    // A partial receipt into the location below zero is accepted; the item stays open (−2).
    queries::transfer_stock(&b, transfer(3.0), None)
        .await
        .expect("partial transfer into a location below zero");
    assert_eq!(on_hand_milli(&b, art_b, loc_b).await, -2000);
    let on_b = negative_stock::list_reviews(&b, true).await.expect("list B");
    assert_eq!(on_b.items.len(), 1);
    assert_eq!(on_b.items[0].status, "open", "still below zero");
    // 5 more: back to 3. Local transfers are not hooked: the row stays open until the list is read.
    queries::transfer_stock(&b, transfer(5.0), None)
        .await
        .expect("transfer back above zero");
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM stock_negative_reviews WHERE article_id = ? AND location_id = ? AND status = 'open'",
            vec![art_b.into(), loc_b.into()],
        )
        .await,
        1,
        "the row is still open before the list is read"
    );
    let on_b = negative_stock::list_reviews(&b, true).await.expect("list B");
    assert_eq!(on_b.items.len(), 1);
    assert_eq!(on_b.items[0].status, "closed");
    assert_eq!(on_b.items[0].closure_kind.as_deref(), Some(CLOSURE_RECEIPT));
    assert_eq!(on_b.items[0].closing_movement_type.as_deref(), Some("TRANSFER_IN"));
    assert_eq!(on_b.items[0].on_hand_qty, 3.0);

    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, article.rowid, location.rowid).await, 3000);
    let on_a = negative_stock::list_reviews(&a, true).await.expect("list A");
    assert_eq!(on_a.items.len(), 1, "A: one item, closed on arrival of the transfer");
    assert_eq!(on_a.items[0].status, "closed");
    assert_eq!(on_a.items[0].closure_kind.as_deref(), Some(CLOSURE_RECEIPT));
    assert_eq!(on_a.items[0].closing_movement_type.as_deref(), Some("TRANSFER_IN"));
}

/// A manual entry made here at a location below zero is a count resolution, not a receipt: it needs
/// a reason of at least 8 characters, is audited, and closes the item as an adjustment.
#[tokio::test]
async fn a_manual_entry_at_a_negative_location_needs_a_reason_and_closes_as_an_adjustment() {
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
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    let (art, loc) = (article.rowid, location.rowid);

    for short in ["test", "   abcdefg   "] {
        let err = adjust_noted(&a, art, loc, 5.0, short)
            .await
            .expect_err("an entry at a location below zero needs its reason");
        assert_eq!(refusal_code(err), REFUSED_ENTRY_REASON_SHORT, "{short:?}");
    }
    assert_eq!(on_hand_milli(&a, art, loc).await, -5000, "refusals write nothing");
    // A partial entry is accepted (the item stays open), the next one brings it back to zero.
    adjust_noted(&a, art, loc, 2.0, "Trouvé 2 au fond du bac")
        .await
        .expect("partial entry");
    assert_eq!(review_rows(&a, art, loc).await, 1);
    adjust_noted(&a, art, loc, 3.0, "Trouvé 3 sur l'étagère")
        .await
        .expect("entry back to zero");
    assert_eq!(on_hand_milli(&a, art, loc).await, 0);
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.negative_stock.adjusted'",
            vec![],
        )
        .await,
        2,
        "each entry at a location below zero is audited"
    );
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM stock_negative_reviews WHERE article_id = ? AND location_id = ? AND status = 'open'",
            vec![art.into(), loc.into()],
        )
        .await,
        0,
        "the entry closes it at once"
    );
    let list = negative_stock::list_reviews(&a, true).await.expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].status, "closed");
    assert_eq!(list.items[0].closure_kind.as_deref(), Some(CLOSURE_ADJUSTMENT));
    assert_eq!(list.items[0].closing_movement_type.as_deref(), Some("ADJUST_IN"));
}

/// Stock returned against the work-order issue it came from is a documented inflow: it closes the
/// item as "régularisé par retour", without a reason.
#[tokio::test]
async fn a_return_against_its_issue_closes_the_item_as_regularised_by_return() {
    use crate::inventory::domain::InventoryReturnInput;
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art, loc) = (article.rowid, location.rowid);
    let reservation = reserve(&a, art, loc, 5.0).await.expect("reserve");
    queries::issue_reserved_stock(
        &a,
        InventoryIssueInput {
            reservation_id: reservation,
            quantity: 5.0,
            source_type: None,
            source_id: None,
            source_ref: None,
            notes: None,
        },
    )
    .await
    .expect("issue");
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art, loc).await, -5000);

    queries::return_reserved_stock(
        &a,
        InventoryReturnInput {
            reservation_id: reservation,
            quantity: 5.0,
            notes: None,
        },
    )
    .await
    .expect("a return into a location below zero is accepted");
    assert_eq!(on_hand_milli(&a, art, loc).await, 0);
    let list = negative_stock::list_reviews(&a, true).await.expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].status, "closed");
    assert_eq!(list.items[0].closure_kind.as_deref(), Some(CLOSURE_RETURN));
    assert_eq!(list.items[0].closing_movement_type.as_deref(), Some("RETURN"));
}

/// One PC reconciles from the list, the other posts a count session for the same location, offline:
/// every count counts, whatever its path — one "double comptage — à vérifier" naming both paths.
#[tokio::test]
async fn a_list_count_and_a_count_session_on_two_pcs_are_a_double_count() {
    use crate::inventory::controls;
    use crate::inventory::domain::{
        ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, PostInventoryCountSessionInput,
        TransitionInventoryCountSessionInput, UpsertInventoryCountLineInput,
    };
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    let manager_a = user_with(&a, "negpatha", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let counter_b = user_with(&b, "negpathb", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    issue(&a, art, loc, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    negative_stock::resolve_by_count(&a, manager_a, resolution(art, loc, 0.0, "Comptage depuis la liste"))
        .await
        .expect("A reconciles from the list");
    // B counts the location through the count workflow.
    let actor = Some(i64::from(counter_b));
    let warehouse_b = scalar_i64(
        &b,
        "SELECT warehouse_id FROM stock_locations WHERE id = ?",
        vec![loc_b.into()],
    )
    .await;
    let mut session = controls::create_count_session(
        &b,
        CreateInventoryCountSessionInput {
            warehouse_id: warehouse_b,
            location_id: Some(loc_b),
            actor_id: actor,
        },
    )
    .await
    .expect("session");
    let move_to =
        |session: &crate::inventory::domain::InventoryCountSession, next: &str| TransitionInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            next_status: next.to_string(),
            reason: None,
            actor_id: actor,
        };
    session = controls::transition_count_session(&b, move_to(&session, "counting"))
        .await
        .expect("counting");
    let line = controls::upsert_count_line(
        &b,
        UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id: art_b,
            location_id: loc_b,
            counted_qty: 0.0,
            variance_reason_code: Some("COUNT_ERROR".into()),
        },
    )
    .await
    .expect("line");
    session = controls::transition_count_session(&b, move_to(&session, "submitted"))
        .await
        .expect("submitted");
    controls::approve_count_line(
        &b,
        ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: i64::from(counter_b),
            reviewer_evidence: "Bac vide au comptage".into(),
        },
    )
    .await
    .expect("approve");
    session = controls::transition_count_session(&b, move_to(&session, "approved"))
        .await
        .expect("approved");
    controls::post_count_session(
        &b,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: actor,
        },
    )
    .await
    .expect("posted");
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    for (pc, db, art, loc) in [("A", &a, art, loc), ("B", &b, art_b, loc_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, 5000, "{pc}: corrected twice");
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert_eq!(list.double_counts.len(), 1, "{pc}: one double count");
        let mut paths: Vec<_> = list.double_counts[0].counts.iter().map(|c| c.path.clone()).collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![
                negative_stock::COUNT_PATH_COUNT_SESSION.to_string(),
                negative_stock::COUNT_PATH_RECONCILIATION.to_string()
            ],
            "{pc}: both paths named"
        );
    }
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
        watch(db).await;
        assert_eq!(notices(db).await.0, 0, "{pc}: no notice for a warning");
    }
}

/// Two PCs both resolve the same item offline (each counted 0 and corrected by +5): after sync the
/// stock is corrected twice (+5) and every PC shows one "double comptage — à vérifier" naming both
/// counts; a new count that knows both closes it everywhere.
#[tokio::test]
async fn two_pcs_resolving_offline_show_a_double_count_that_a_new_count_closes() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    let manager_a = user_with(&a, "negdoublea", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let manager_b = user_with(&b, "negdoubleb", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    issue(&a, art, loc, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    // Offline, each PC counts 0 and resolves its item.
    negative_stock::resolve_by_count(&a, manager_a, resolution(art, loc, 0.0, "Comptage poste A"))
        .await
        .expect("A resolves");
    negative_stock::resolve_by_count(&b, manager_b, resolution(art_b, loc_b, 0.0, "Comptage poste B"))
        .await
        .expect("B resolves");
    watch(&a).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;

    let device_a = crate::sync::device_code::this_device_code(&a).await.expect("code A");
    let device_b = crate::sync::device_code::this_device_code(&b).await.expect("code B");
    assert_ne!(device_a, device_b);
    for (pc, db, art, loc) in [("A", &a, art, loc), ("B", &b, art_b, loc_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, 5000, "{pc}: corrected twice");
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert!(list.items.is_empty(), "{pc}: no longer below zero");
        assert_eq!(list.double_counts.len(), 1, "{pc}: one double count");
        let double = &list.double_counts[0];
        assert_eq!((double.article_id, double.location_id), (art, loc), "{pc}");
        let mut devices: Vec<_> = double.counts.iter().map(|c| c.device_code.clone()).collect();
        devices.sort();
        let mut expected = vec![device_a.clone(), device_b.clone()];
        expected.sort();
        assert_eq!(devices, expected, "{pc}: both PCs named");
        let mut who: Vec<_> = double.counts.iter().filter_map(|c| c.counted_by.clone()).collect();
        who.sort();
        assert_eq!(who, vec!["negdoublea".to_string(), "negdoubleb".to_string()], "{pc}");
        assert!(double
            .counts
            .iter()
            .all(|c| c.counted_qty == 0.0 && !c.counted_at.is_empty()));
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").double_count_count,
            1,
            "{pc}"
        );
        let report = watch(db).await;
        assert_eq!(
            report.raised + report.already_true,
            1,
            "{pc}: one notice for the double count"
        );
    }

    let short = negative_stock::resolve_by_count(&a, manager_a, resolution(art, loc, 0.0, "Recompt"))
        .await
        .expect_err("7 characters");
    assert_eq!(refusal_code(short), REFUSED_REASON_SHORT);
    let closed = negative_stock::resolve_by_count(&a, manager_a, resolution(art, loc, 0.0, "Recomptage commun"))
        .await
        .expect("a new count closes it");
    assert!(closed.review.is_none(), "no negative item was open");
    assert_eq!(closed.variance_qty, -5.0);
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    for (pc, db, art, loc) in [("A", &a, art, loc), ("B", &b, art_b, loc_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, 0, "{pc}");
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert!(list.double_counts.is_empty(), "{pc}: closed by the new count");
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").double_count_count,
            0,
            "{pc}"
        );
    }
    let again = negative_stock::resolve_by_count(&a, manager_a, resolution(art, loc, 0.0, "Recomptage commun"))
        .await
        .expect_err("nothing left");
    assert_eq!(refusal_code(again), REFUSED_NOT_OPEN);
}

/// A location below zero refuses local reservations, issues (also of stock reserved before) and
/// exits (available-stock checks), and the count that resolves it needs `inv.manage` and a reason
/// of at least 8 characters once trimmed.
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

    let denied = negative_stock::resolve_by_count(&a, viewer, resolution(art, loc, 0.0, "Recompté bac"))
        .await
        .expect_err("inv.view only");
    assert!(matches!(denied, AppError::PermissionDenied(_)), "{denied:?}");
    for short in ["", "        ", "abcdefg", "  abcd  "] {
        let err = negative_stock::resolve_by_count(&a, manager, resolution(art, loc, 0.0, short))
            .await
            .expect_err("short reason");
        assert_eq!(refusal_code(err), REFUSED_REASON_SHORT, "{short:?}");
    }
    assert_eq!(review_rows(&a, art, loc).await, 1);
    assert_eq!(on_hand_milli(&a, art, loc).await, -4000, "refusals write nothing");

    // Counted 0 while 1 is still reserved: the item closes, the reservation shows as a warning.
    let resolved = negative_stock::resolve_by_count(&a, manager, resolution(art, loc, 0.0, " abcdefgh "))
        .await
        .expect("eight characters once trimmed");
    let closed = resolved.review.expect("the negative item");
    assert_eq!(closed.status, "closed");
    assert_eq!(
        (closed.on_hand_qty, closed.reserved_qty, closed.available_qty),
        (0.0, 1.0, -1.0)
    );
    let list = negative_stock::list_reviews(&a, false).await.expect("list");
    assert!(list.items.is_empty());
    assert_eq!(list.over_reservations.len(), 1, "over-reservation warning");
    let again = negative_stock::resolve_by_count(&a, manager, resolution(art, loc, 2.0, "Recompté bac"))
        .await
        .expect_err("nothing left to reconcile");
    assert_eq!(refusal_code(again), REFUSED_NOT_OPEN);
}
