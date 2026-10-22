//! Stock to reconcile after sync (`inventory::negative_stock`, CMMS Advisor 2026-10-20 and ruling
//! S1): two PCs issuing the last units, the one review item per article and location whatever the
//! arrival order, its notice, its resolution by a count, its closure by a transfer in or a manual
//! entry, the over-reservation warning, the double count of two PCs resolving offline, and the
//! local refusals.

use sea_orm::DatabaseConnection;

use crate::errors::AppError;
use crate::inventory::domain::{InventoryIssueInput, InventoryReserveInput, InventoryStockAdjustInput};
use crate::inventory::negative_stock::{
    self, ResolveNegativeStockInput, CLOSURE_ADJUSTMENT, CLOSURE_COUNT, CLOSURE_PRIOR_ENTRY, CLOSURE_RECEIPT,
    CLOSURE_RETURN, REFUSED_ENTRY_REASON_SHORT, REFUSED_NOT_OPEN, REFUSED_REASON_SHORT,
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

/// A count session of the location on `db`, in counting, with its line entered at `counted` (the
/// variance and the count's mark are fixed now).
async fn enter_count(
    db: &DatabaseConnection,
    user: i32,
    article_id: i64,
    location_id: i64,
    counted: f64,
) -> (
    crate::inventory::domain::InventoryCountSession,
    crate::inventory::domain::InventoryCountLine,
) {
    use crate::inventory::controls;
    use crate::inventory::domain::{CreateInventoryCountSessionInput, UpsertInventoryCountLineInput};
    let warehouse_id = scalar_i64(
        db,
        "SELECT warehouse_id FROM stock_locations WHERE id = ?",
        vec![location_id.into()],
    )
    .await;
    let session = controls::create_count_session(
        db,
        CreateInventoryCountSessionInput {
            warehouse_id,
            location_id: Some(location_id),
            actor_id: Some(i64::from(user)),
        },
    )
    .await
    .expect("session");
    let session = controls::transition_count_session(db, move_to(&session, "counting", user))
        .await
        .expect("counting");
    let line = controls::upsert_count_line(
        db,
        UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id,
            location_id,
            counted_qty: counted,
            variance_reason_code: Some("COUNT_ERROR".into()),
        },
    )
    .await
    .expect("line");
    (session, line)
}

fn move_to(
    session: &crate::inventory::domain::InventoryCountSession,
    next: &str,
    user: i32,
) -> crate::inventory::domain::TransitionInventoryCountSessionInput {
    crate::inventory::domain::TransitionInventoryCountSessionInput {
        session_id: session.id,
        expected_row_version: session.row_version,
        next_status: next.to_string(),
        reason: None,
        actor_id: Some(i64::from(user)),
    }
}

/// Submits, approves (when the line has a variance) and posts a session from [`enter_count`].
async fn post_count(
    db: &DatabaseConnection,
    user: i32,
    (session, line): (
        crate::inventory::domain::InventoryCountSession,
        crate::inventory::domain::InventoryCountLine,
    ),
) -> crate::inventory::domain::InventoryCountSession {
    use crate::inventory::controls;
    use crate::inventory::domain::{ApproveInventoryCountLineInput, PostInventoryCountSessionInput};
    let session = controls::transition_count_session(db, move_to(&session, "submitted", user))
        .await
        .expect("submitted");
    if line.approval_required != 0 {
        controls::approve_count_line(
            db,
            ApproveInventoryCountLineInput {
                line_id: line.id,
                expected_row_version: line.row_version,
                reviewer_id: i64::from(user),
                reviewer_evidence: "Bac vérifié au comptage".into(),
            },
        )
        .await
        .expect("approve");
    }
    let session = controls::transition_count_session(db, move_to(&session, "approved", user))
        .await
        .expect("approved");
    controls::post_count_session(
        db,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(i64::from(user)),
        },
    )
    .await
    .expect("posted")
}

/// `(closure kind, closing movement type)` of the one review item of a pair, after the list is read.
async fn closed_item(db: &DatabaseConnection, article_id: i64, location_id: i64) -> (String, String) {
    let list = negative_stock::list_reviews(db, true).await.expect("list");
    let items: Vec<_> = list
        .items
        .iter()
        .filter(|i| i.article_id == article_id && i.location_id == location_id)
        .collect();
    assert_eq!(items.len(), 1, "one item");
    assert_eq!(items[0].status, "closed");
    (
        items[0].closure_kind.clone().unwrap_or_default(),
        items[0].closing_movement_type.clone().unwrap_or_default(),
    )
}

/// Stock 5 at a location known to A, B (and C when `three`), which A and B each issue entirely:
/// after sync every PC stands at −5 with an open item. Returns the PCs, the server and each PC's
/// local `(article, location)`.
struct Shortage {
    dbs: Vec<DatabaseConnection>,
    server: TestServer,
    pairs: Vec<(i64, i64)>,
}

async fn shortage(pcs: usize) -> Shortage {
    let mut dbs = Vec::new();
    for _ in 0..pcs {
        dbs.push(started_device_db().await);
    }
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&dbs[0], &mut factory, 5.0).await;
    let names = ["A", "B", "C"];
    for (i, db) in dbs.iter().enumerate() {
        sync_clean(&mut server, names[i], db).await;
    }
    let mut pairs = Vec::new();
    for db in &dbs {
        pairs.push(local_pair(db, &article, &location).await);
    }
    issue(&dbs[0], pairs[0].0, pairs[0].1, 5.0).await;
    issue(&dbs[1], pairs[1].0, pairs[1].1, 5.0).await;
    for round in 0..2 {
        for (i, db) in dbs.iter().enumerate() {
            if round == 1 && i == 1 {
                continue;
            }
            sync_clean(&mut server, names[i], db).await;
        }
    }
    for (i, db) in dbs.iter().enumerate() {
        assert_eq!(on_hand_milli(db, pairs[i].0, pairs[i].1).await, -5000, "{}", names[i]);
    }
    Shortage { dbs, server, pairs }
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
               AND cl.count_follows_json = '[]' AND cl.count_replica_id = ?
               AND cl.count_path = 'reconciliation'",
            vec![
                art_a.into(),
                loc_a.into(),
                crate::sync::queries::local_replica_id(&a)
                    .await
                    .expect("replica")
                    .into(),
            ],
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
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM inventory_transactions WHERE movement_type = 'ADJUST_IN' AND source_type = ?",
            vec![negative_stock::SOURCE_NEGATIVE_STOCK_ENTRY.into()],
        )
        .await,
        2,
        "both entries are marked as made below zero, on the movement every PC receives"
    );
}

/// Every movement that can raise the on-hand stock has its closure kind, so "other" (no stock entry
/// on record) is never shown when one was recorded; the repair-order cancellation is a return.
#[test]
fn every_on_hand_entry_has_its_closure_kind() {
    use crate::inventory::negative_stock::{closure_kind_of, CLOSURE_PRIOR_ENTRY, SOURCE_NEGATIVE_STOCK_ENTRY};
    for (movement, sign) in crate::inventory::ledger::ON_HAND_MOVEMENTS {
        // COUNT_ADJUST and COUNT_REVERSAL carry a signed quantity: they raise the stock too.
        let raises = *sign > 0.0;
        for documented in [true, false] {
            for source in [None, Some(SOURCE_NEGATIVE_STOCK_ENTRY), Some("MANUAL_ADJUSTMENT")] {
                assert_eq!(
                    closure_kind_of(movement, documented, source).is_some(),
                    raises,
                    "{movement} documented={documented} source={source:?}"
                );
            }
        }
    }
    assert_eq!(closure_kind_of("ISSUE", true, None), None);
    assert_eq!(closure_kind_of("COUNT_ADJUST", false, None), Some(CLOSURE_COUNT));
    assert_eq!(closure_kind_of("COUNT_REVERSAL", false, None), Some(CLOSURE_ADJUSTMENT));
    assert_eq!(
        closure_kind_of("ADJUST_IN", false, Some(SOURCE_NEGATIVE_STOCK_ENTRY)),
        Some(CLOSURE_ADJUSTMENT)
    );
    assert_eq!(
        closure_kind_of("ADJUST_IN", false, Some("MANUAL_ADJUSTMENT")),
        Some(CLOSURE_PRIOR_ENTRY)
    );
    for movement in ["GR_ACCEPT", "TRANSFER_IN"] {
        assert_eq!(closure_kind_of(movement, false, None), Some(CLOSURE_RECEIPT));
    }
    for movement in ["RETURN", "REPAIRABLE_RETURN", "REPAIRABLE_CANCEL_RESTORE"] {
        assert_eq!(
            closure_kind_of(movement, true, None),
            Some(CLOSURE_RETURN),
            "{movement}"
        );
        assert_eq!(
            closure_kind_of(movement, false, None),
            Some(CLOSURE_ADJUSTMENT),
            "{movement}"
        );
    }
}

/// B2(a): B enters its count at −5 (variance +5), then receives A's count by sync (stock 0), then
/// posts: the stock goes to +5. B's count was fixed without knowing A's, so it is a double count,
/// even though B knew A's count when it posted.
#[tokio::test]
async fn a_count_entered_before_the_other_pcs_count_arrived_is_a_double_count() {
    let Shortage {
        dbs, mut server, pairs, ..
    } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let manager_a = user_with(a, "negstalea", &[crate::rbac::permissions::INV_MANAGE]).await;
    // The test accounts share org-structure rows: B receives A's before making its own.
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    let counter_b = user_with(b, "negstaleb", &[crate::rbac::permissions::INV_MANAGE]).await;

    let entered = enter_count(b, counter_b, pairs[1].0, pairs[1].1, 0.0).await;
    assert_eq!((entered.1.system_qty, entered.1.variance_qty), (-5.0, 5.0));
    negative_stock::resolve_by_count(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A reconciles from the list");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    assert_eq!(on_hand_milli(b, pairs[1].0, pairs[1].1).await, 0, "B has A's count");
    post_count(b, counter_b, entered).await;
    sync_clean(&mut server, "B", b).await;
    sync_clean(&mut server, "A", a).await;

    for (pc, db, (art, loc)) in [("A", a, pairs[0]), ("B", b, pairs[1])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 5000, "{pc}: corrected twice");
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert_eq!(list.double_counts.len(), 1, "{pc}: one double count");
        let counts = &list.double_counts[0].counts;
        assert_eq!(counts.len(), 2, "{pc}");
        assert!(
            counts.iter().all(|c| c.system_qty == -5.0 && c.variance_qty == 5.0),
            "{pc}"
        );
        let mut who: Vec<_> = counts.iter().filter_map(|c| c.counted_by.clone()).collect();
        who.sort();
        assert_eq!(who, vec!["negstalea".to_string(), "negstaleb".to_string()], "{pc}");
    }
    let replica_b = crate::sync::queries::local_replica_id(b).await.expect("replica B");
    assert_eq!(
        scalar_i64(
            b,
            "SELECT COUNT(*) FROM inventory_count_lines WHERE count_replica_id = ? AND count_follows_json = '[]'",
            vec![replica_b.into()],
        )
        .await,
        1,
        "B's mark was taken at entry, before A's count arrived"
    );
}

/// B2(b): A counts at −5, then makes a zero-variance cycle count at 0; C, offline throughout, counts
/// at −5. The later count at 0 does not hide A's first count: A's and C's counts never knew each
/// other, so it is a double count on every PC, until a count that knows both.
#[tokio::test]
async fn a_zero_variance_count_does_not_hide_an_earlier_count_below_zero() {
    let Shortage {
        dbs, mut server, pairs, ..
    } = shortage(3).await;
    let (a, c) = (&dbs[0], &dbs[2]);
    let manager_a = user_with(a, "negcyclea", &[crate::rbac::permissions::INV_MANAGE]).await;
    // The test accounts share org-structure rows: C receives A's before making its own.
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "C", c).await;
    let manager_c = user_with(c, "negcyclec", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art_a, loc_a) = pairs[0];
    let (art_c, loc_c) = pairs[2];

    negative_stock::resolve_by_count(a, manager_a, resolution(art_a, loc_a, 0.0, "Comptage poste A"))
        .await
        .expect("A1 at −5");
    let cycle = enter_count(a, manager_a, art_a, loc_a, 0.0).await;
    assert_eq!((cycle.1.system_qty, cycle.1.variance_qty), (0.0, 0.0), "A2 at 0");
    post_count(a, manager_a, cycle).await;
    negative_stock::resolve_by_count(c, manager_c, resolution(art_c, loc_c, 0.0, "Comptage poste C"))
        .await
        .expect("C1 at −5, offline");
    for _ in 0..2 {
        sync_clean(&mut server, "A", a).await;
        sync_clean(&mut server, "C", c).await;
    }

    for (pc, db, (art, loc)) in [("A", a, pairs[0]), ("C", c, pairs[2])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 5000, "{pc}: corrected twice");
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert_eq!(list.double_counts.len(), 1, "{pc}: the count at 0 hides nothing");
        let counts = &list.double_counts[0].counts;
        let mut who: Vec<_> = counts.iter().filter_map(|c| c.counted_by.clone()).collect();
        who.sort();
        assert_eq!(who, vec!["negcyclea".to_string(), "negcyclec".to_string()], "{pc}");
        assert!(
            counts.iter().all(|c| c.system_qty == -5.0),
            "{pc}: only the counts below zero"
        );
    }

    negative_stock::resolve_by_count(a, manager_a, resolution(art_a, loc_a, 0.0, "Recomptage commun"))
        .await
        .expect("a count that knows both");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "C", c).await;
    for (pc, db, (art, loc)) in [("A", a, pairs[0]), ("C", c, pairs[2])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 0, "{pc}");
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert!(
            list.double_counts.is_empty(),
            "{pc}: closed by the count that knows both"
        );
    }
}

/// The notice of a negative item is the article and location: a movement dated earlier that arrives
/// late moves the crossing in the ledger order, and the notice is neither raised again nor resolved.
#[tokio::test]
async fn a_late_movement_dated_earlier_does_not_repeat_the_notice() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let c = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "C", &c).await;
    let (art_a, loc_a) = (article.rowid, location.rowid);
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art_c, loc_c) = local_pair(&c, &article, &location).await;
    watch(&a).await;

    // A issues 5 at 08:00, B 3 at 08:02, C 2 at 08:01 (C's arrives last).
    for (db, art, loc, qty, at) in [
        (&a, art_a, loc_a, 5.0, "2099-01-01T08:00:00Z"),
        (&b, art_b, loc_b, 3.0, "2099-01-01T08:02:00Z"),
        (&c, art_c, loc_c, 2.0, "2099-01-01T08:01:00Z"),
    ] {
        issue(db, art, loc, qty).await;
        // Dated before it is sent (capture suppressed: the movement is still sent once, as made,
        // with this date, as from a PC whose clock or entry date says so).
        use sea_orm::TransactionTrait;
        let tx = db.begin().await.expect("tx");
        crate::sync::capture::begin_suppression(&tx).await.expect("suppress");
        exec(
            &tx,
            "UPDATE inventory_transactions SET performed_at = ?
             WHERE movement_type = 'ISSUE' AND article_id = ? AND location_id = ?",
            vec![at.into(), art.into(), loc.into()],
        )
        .await;
        crate::sync::capture::end_suppression(&tx).await.expect("unsuppress");
        tx.commit().await.expect("commit");
    }
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art_a, loc_a).await, -3000);
    let first = watch(&a).await;
    assert_eq!(first.raised + first.already_true, 1, "one notice");
    let crossing_before = negative_stock::negative_crossing(&a, art_a, loc_a, -3.0)
        .await
        .expect("crossing")
        .expect("below zero after a crossing")
        .0;

    sync_clean(&mut server, "C", &c).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art_a, loc_a).await, -5000);
    let crossing_after = negative_stock::negative_crossing(&a, art_a, loc_a, -5.0)
        .await
        .expect("crossing")
        .expect("below zero after a crossing")
        .0;
    assert_ne!(crossing_before, crossing_after, "the late movement moved the crossing");
    let again = watch(&a).await;
    assert_eq!(
        (again.raised, again.already_true, again.resolved),
        (0, 0, 0),
        "neither raised again nor resolved"
    );
    assert_eq!(notices(&a).await.0, 1, "still one notice");
}

/// An entry made on a PC that still saw the location at zero or more asks no reason; when it later
/// closes the item on another PC it is "régularisé par saisie antérieure", not an adjustment.
#[tokio::test]
async fn an_entry_made_before_the_shortage_was_visible_closes_as_a_prior_entry() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art, loc) = (article.rowid, location.rowid);
    issue(&a, art, loc, 5.0).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(review_rows(&a, art, loc).await, 1, "A: item open");
    assert_eq!(on_hand_milli(&b, art_b, loc_b).await, 0, "B has not seen A's issue");

    adjust_noted(&b, art_b, loc_b, 5.0, "test")
        .await
        .expect("no reason asked at zero");
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art, loc).await, 0);
    assert_eq!(
        closed_item(&a, art, loc).await,
        (CLOSURE_PRIOR_ENTRY.to_string(), "ADJUST_IN".to_string())
    );
}

/// Cancelling a repair order after its release puts the part back where it was taken, naming the
/// repair order: it closes the item as "régularisé par retour".
#[tokio::test]
async fn a_cancelled_repair_release_closes_the_item_as_a_return() {
    use crate::inventory::domain::{CreateRepairableOrderInput, TransitionRepairableOrderInput};
    use crate::inventory::procurement;
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art, loc) = (article.rowid, location.rowid);
    let transition = |order_id: i64, row_version: i64, next: &str| TransitionRepairableOrderInput {
        order_id,
        expected_row_version: row_version,
        next_status: next.to_string(),
        reason: Some("Annulation de la réparation".to_string()),
        note: None,
        actor_id: None,
        return_location_id: Some(loc),
        serial_number: None,
        vendor_supplier_id: None,
        repair_cost: None,
        warranty_active: None,
        warranty_until: None,
    };
    let order = procurement::create_repairable_order(
        &a,
        CreateRepairableOrderInput {
            article_id: art,
            quantity: 2.0,
            source_location_id: loc,
            return_location_id: Some(loc),
            linked_po_line_id: None,
            linked_reservation_id: None,
            reason: Some("Pompe à réparer".to_string()),
            serial_number: None,
            vendor_supplier_id: None,
            actor_id: None,
        },
    )
    .await
    .expect("repair order");
    let order = procurement::transition_repairable_order(&a, transition(order.id, order.row_version, "RELEASED"))
        .await
        .expect("released");
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art, loc).await, -2000);

    procurement::transition_repairable_order(&a, transition(order.id, order.row_version, "CANCELLED"))
        .await
        .expect("cancelled after release: accepted at a location below zero");
    assert_eq!(on_hand_milli(&a, art, loc).await, 0);
    assert_eq!(
        closed_item(&a, art, loc).await,
        (CLOSURE_RETURN.to_string(), "REPAIRABLE_CANCEL_RESTORE".to_string())
    );
}

/// Reversing a count can bring a location below zero back to zero or more: that closes the item as
/// an adjustment (not a count), and the reversal needs a reason of at least 8 characters, trimmed.
#[tokio::test]
async fn a_count_reversal_closes_the_item_as_an_adjustment_with_its_reason() {
    use crate::inventory::controls;
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 5.0).await;
    let manager = user_with(&a, "negreversal", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art, loc) = (article.rowid, location.rowid);

    // A counts 2 where 5 were on record (−3), then B's issue of 5 arrives: −3.
    let entered = enter_count(&a, manager, art, loc, 2.0).await;
    let session = post_count(&a, manager, entered).await;
    issue(&b, art_b, loc_b, 5.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art, loc).await, -3000);
    assert_eq!(review_rows(&a, art, loc).await, 1);

    let reverse = |reason: &str| ReverseInventoryCountSessionInput {
        session_id: session.id,
        expected_row_version: session.row_version,
        reason: reason.to_string(),
        actor_id: Some(i64::from(manager)),
    };
    for short in ["Erreur", "  abcdefg  ", "éèàùç"] {
        assert!(
            controls::reverse_count_session(&a, reverse(short)).await.is_err(),
            "{short:?}: shorter than 8 characters once trimmed"
        );
    }
    controls::reverse_count_session(&a, reverse("  Mauvais bac compté  "))
        .await
        .expect("reversal");
    assert_eq!(on_hand_milli(&a, art, loc).await, 0);
    assert_eq!(
        closed_item(&a, art, loc).await,
        (CLOSURE_ADJUSTMENT.to_string(), "COUNT_REVERSAL".to_string())
    );
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
    let entered = enter_count(&b, counter_b, art_b, loc_b, 0.0).await;
    post_count(&b, counter_b, entered).await;
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

/// Runs `sql` on `db` with change capture suppressed: a local repair the other PCs never receive.
async fn exec_unsent(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    use sea_orm::TransactionTrait;
    let tx = db.begin().await.expect("tx");
    crate::sync::capture::begin_suppression(&tx).await.expect("suppress");
    exec(&tx, sql, values).await;
    crate::sync::capture::end_suppression(&tx).await.expect("unsuppress");
    tx.commit().await.expect("commit");
}

/// The count session `code` on `db`.
async fn session_by_code(db: &DatabaseConnection, code: &str) -> crate::inventory::domain::InventoryCountSession {
    crate::inventory::controls::list_count_sessions(db)
        .await
        .expect("sessions")
        .into_iter()
        .find(|s| s.session_code == code)
        .expect("session")
}

/// SF1: a count line received before its stock movement is not followed by a new count's mark: the
/// new count never "knows" a correction its stock does not include.
#[tokio::test]
async fn a_count_whose_movement_is_not_here_is_not_followed() {
    let Shortage { dbs, mut server, pairs } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let manager_a = user_with(a, "negunapplieda", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    let counter_b = user_with(b, "negunappliedb", &[crate::rbac::permissions::INV_MANAGE]).await;
    negative_stock::resolve_by_count(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A1");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    // As if A1's line had arrived on B but not yet its movement.
    exec_unsent(
        b,
        "UPDATE inventory_count_lines SET posted_transaction_id = NULL WHERE count_path = 'reconciliation'",
        vec![],
    )
    .await;
    let entered = enter_count(b, counter_b, pairs[1].0, pairs[1].1, 0.0).await;
    let follows = crate::sync::test_support::scalar_opt_text(
        b,
        "SELECT count_follows_json FROM inventory_count_lines WHERE id = ?",
        vec![entered.1.id.into()],
    )
    .await;
    assert_eq!(
        follows.as_deref(),
        Some("[]"),
        "A1's movement is not counted here: not followed"
    );
}

/// SF3: B enters at −5, receives A's count, enters the line again: it is now set against the
/// corrected stock and knows A's count, so posting it is no double count.
#[tokio::test]
async fn a_count_entered_again_after_the_other_count_arrived_is_no_double_count() {
    let Shortage { dbs, mut server, pairs } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let manager_a = user_with(a, "negreentera", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    let counter_b = user_with(b, "negreenterb", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (session, first) = enter_count(b, counter_b, pairs[1].0, pairs[1].1, 0.0).await;
    assert_eq!(first.system_qty, -5.0);
    negative_stock::resolve_by_count(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A1");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    let again = crate::inventory::controls::upsert_count_line(
        b,
        crate::inventory::domain::UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id: pairs[1].0,
            location_id: pairs[1].1,
            counted_qty: 0.0,
            variance_reason_code: Some("COUNT_ERROR".into()),
        },
    )
    .await
    .expect("entered again");
    assert_eq!((again.system_qty, again.variance_qty), (0.0, 0.0));
    post_count(b, counter_b, (session, again)).await;
    sync_clean(&mut server, "B", b).await;
    sync_clean(&mut server, "A", a).await;
    for (pc, db, (art, loc)) in [("A", a, pairs[0]), ("B", b, pairs[1])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 0, "{pc}: corrected once");
        let list = negative_stock::list_reviews(db, true).await.expect("list");
        assert!(list.double_counts.is_empty(), "{pc}: no double count");
    }
}

/// Advisor: on one PC, a session whose location was counted and posted since it was opened is
/// refused at posting (its snapshot is stale); it is counted again from a fresh snapshot.
#[tokio::test]
async fn a_session_counted_after_another_count_on_the_same_pc_is_refused() {
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let b = &dbs[1];
    let counter_b = user_with(b, "negstalesession", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = pairs[1];
    let stale = enter_count(b, counter_b, art, loc, 0.0).await;
    negative_stock::resolve_by_count(b, counter_b, resolution(art, loc, 0.0, "Comptage depuis la liste"))
        .await
        .expect("a count posted from the list on the same PC");
    assert_eq!(on_hand_milli(b, art, loc).await, 0);

    use crate::inventory::controls;
    let session = controls::transition_count_session(b, move_to(&stale.0, "submitted", counter_b))
        .await
        .expect("submitted");
    controls::approve_count_line(
        b,
        crate::inventory::domain::ApproveInventoryCountLineInput {
            line_id: stale.1.id,
            expected_row_version: stale.1.row_version,
            reviewer_id: i64::from(counter_b),
            reviewer_evidence: "Bac vérifié au comptage".into(),
        },
    )
    .await
    .expect("approve");
    let session = controls::transition_count_session(b, move_to(&session, "approved", counter_b))
        .await
        .expect("approved");
    let refused = controls::post_count_session(
        b,
        crate::inventory::domain::PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(i64::from(counter_b)),
        },
    )
    .await
    .expect_err("stale snapshot");
    assert_eq!(refusal_code(refused), controls::REFUSED_COUNT_SESSION_STALE);
    assert_eq!(on_hand_milli(b, art, loc).await, 0, "nothing corrected twice");

    // Counted again in a new session, from today's stock: posted.
    let fresh = enter_count(b, counter_b, art, loc, 0.0).await;
    assert_eq!(fresh.1.variance_qty, 0.0);
    post_count(b, counter_b, fresh).await;
}

/// Advisor: a reversal that raises a location below zero without bringing it back to zero is
/// accepted, with its reason and an audit event; the item stays open.
#[tokio::test]
async fn a_reversal_that_leaves_the_location_below_zero_is_accepted_and_the_item_stays_open() {
    use crate::inventory::controls;
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    let manager = user_with(&a, "negpartialrev", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    let (art, loc) = (article.rowid, location.rowid);

    // A counts 8 where 10 were on record (−2), issues the 8; B's issue of 10 arrives: −10.
    let session = post_count(&a, manager, enter_count(&a, manager, art, loc, 8.0).await).await;
    issue(&a, art, loc, 8.0).await;
    issue(&b, art_b, loc_b, 10.0).await;
    sync_clean(&mut server, "B", &b).await;
    sync_clean(&mut server, "A", &a).await;
    assert_eq!(on_hand_milli(&a, art, loc).await, -10000);

    let reverse = |reason: &str| ReverseInventoryCountSessionInput {
        session_id: session.id,
        expected_row_version: session.row_version,
        reason: reason.to_string(),
        actor_id: Some(i64::from(manager)),
    };
    assert!(
        controls::reverse_count_session(&a, reverse("abcdefg")).await.is_err(),
        "7 characters"
    );
    controls::reverse_count_session(&a, reverse("Mauvais bac compté"))
        .await
        .expect("a reversal that improves a location below zero");
    assert_eq!(on_hand_milli(&a, art, loc).await, -8000, "still below zero");
    let list = negative_stock::list_reviews(&a, false).await.expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].status, "open", "the item stays open");
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.negative_stock.count_reversed' AND actor_id = ?",
            vec![i64::from(manager).into()],
        )
        .await,
        1,
        "audited"
    );
}

/// Advisor (SF2): reversing one of the two counts of a double count closes it, noted with the
/// reversed count, who reversed it and when; the stock is then corrected once. If it is still below
/// zero, the location's negative item stays open.
#[tokio::test]
async fn reversing_one_of_the_two_counts_closes_the_double_count() {
    use crate::inventory::controls;
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let Shortage { dbs, mut server, pairs } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let manager_a = user_with(a, "negrevdoublea", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    let manager_b = user_with(b, "negrevdoubleb", &[crate::rbac::permissions::INV_MANAGE]).await;
    let resolved_a = negative_stock::resolve_by_count(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A1");
    negative_stock::resolve_by_count(
        b,
        manager_b,
        resolution(pairs[1].0, pairs[1].1, 0.0, "Comptage poste B"),
    )
    .await
    .expect("B1");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    sync_clean(&mut server, "A", a).await;
    assert_eq!(
        negative_stock::list_reviews(a, false)
            .await
            .expect("list")
            .double_counts
            .len(),
        1
    );

    let session = session_by_code(a, &resolved_a.session_code).await;
    controls::reverse_count_session(
        a,
        ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Compté en double avec le poste B".into(),
            actor_id: Some(i64::from(manager_a)),
        },
    )
    .await
    .expect("reversal of A1");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    for (pc, db, (art, loc)) in [("A", a, pairs[0]), ("B", b, pairs[1])] {
        assert_eq!(on_hand_milli(db, art, loc).await, 0, "{pc}: corrected once");
        let list = negative_stock::list_reviews(db, true).await.expect("list");
        assert!(list.double_counts.is_empty(), "{pc}: closed");
        assert_eq!(list.closed_double_counts.len(), 1, "{pc}: kept as closed");
        let closed = &list.closed_double_counts[0];
        let by = closed.closed_by_reversal.as_ref().expect("closed by the reversal");
        assert_eq!(by.session_code, resolved_a.session_code, "{pc}");
        assert_eq!(by.reversed_by.as_deref(), Some("negrevdoublea"), "{pc}");
        assert!(!by.reversed_at.is_empty(), "{pc}");
        assert_eq!(closed.counts.len(), 2, "{pc}: both counts named");
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").double_count_count,
            0
        );
        assert!(negative_stock::list_reviews(db, false)
            .await
            .expect("list")
            .closed_double_counts
            .is_empty());
    }
}

/// Advisor: the notice of a location below zero says when the current shortage opened; a later
/// shortage of the same location reopens the same notice with its own date.
#[tokio::test]
async fn the_notice_says_when_the_shortage_opened_and_a_new_shortage_reopens_it_with_its_date() {
    let Shortage { dbs, mut server, pairs } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let manager = user_with(a, "negnoticedate", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = pairs[0];
    let params = || async {
        crate::sync::test_support::scalar_opt_text(
            a,
            "SELECT message_params_json FROM notification_events WHERE category_code = ?",
            vec![negative_stock::NOTIFICATION_CATEGORY.into()],
        )
        .await
        .map(|p| serde_json::from_str::<serde_json::Value>(&p).expect("params"))
        .expect("one notice")
    };
    watch(a).await;
    let first_crossing = negative_stock::negative_crossing(a, art, loc, -5.0)
        .await
        .expect("crossing")
        .and_then(|(_, at)| at)
        .expect("dated");
    assert_eq!(params().await["opened_at"], serde_json::json!(first_crossing));

    negative_stock::resolve_by_count(a, manager, resolution(art, loc, 0.0, "Comptage poste A"))
        .await
        .expect("count");
    assert_eq!(watch(a).await.resolved, 1);
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;

    // A new shortage, dated later: A stocks 3 and issues them, B's issue of 3 arrives.
    adjust(a, art, loc, 3.0).await.expect("stock in");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    issue(a, art, loc, 3.0).await;
    issue(b, pairs[1].0, pairs[1].1, 3.0).await;
    exec_unsent(
        b,
        "UPDATE inventory_transactions SET performed_at = '2099-01-01T09:00:00Z'
         WHERE movement_type = 'ISSUE' AND article_id = ? AND location_id = ? AND performed_at < '2099'",
        vec![pairs[1].0.into(), pairs[1].1.into()],
    )
    .await;
    sync_clean(&mut server, "B", b).await;
    sync_clean(&mut server, "A", a).await;
    assert_eq!(on_hand_milli(a, art, loc).await, -3000);
    let again = watch(a).await;
    assert_eq!(again.raised + again.already_true, 1, "the notice comes back");
    assert_eq!(notices(a).await.0, 1, "the same notice, reopened");
    assert_eq!(params().await["opened_at"], serde_json::json!("2099-01-01T09:00:00Z"));
}

/// Advisor (SF2): a double count closed by a reversal while the stock is still below zero: the
/// double count is closed, the location's "à rapprocher" item stays open.
#[tokio::test]
async fn a_double_count_closed_by_a_reversal_leaves_the_negative_item_open() {
    use crate::inventory::controls;
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let Shortage { dbs, mut server, pairs } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let manager_a = user_with(a, "negrevopena", &[crate::rbac::permissions::INV_MANAGE]).await;
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    let manager_b = user_with(b, "negrevopenb", &[crate::rbac::permissions::INV_MANAGE]).await;
    let resolved_a = negative_stock::resolve_by_count(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A1");
    negative_stock::resolve_by_count(
        b,
        manager_b,
        resolution(pairs[1].0, pairs[1].1, 0.0, "Comptage poste B"),
    )
    .await
    .expect("B1");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    sync_clean(&mut server, "A", a).await;
    assert_eq!(on_hand_milli(a, pairs[0].0, pairs[0].1).await, 5000, "corrected twice");

    // Offline: B issues the 5 it shows; A reverses its count (5 → 0 on A).
    issue(b, pairs[1].0, pairs[1].1, 5.0).await;
    let session = session_by_code(a, &resolved_a.session_code).await;
    controls::reverse_count_session(
        a,
        ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Compté en double avec le poste B".into(),
            actor_id: Some(i64::from(manager_a)),
        },
    )
    .await
    .expect("reversal of A1");
    sync_clean(&mut server, "A", a).await;
    sync_clean(&mut server, "B", b).await;
    sync_clean(&mut server, "A", a).await;
    for (pc, db, (art, loc)) in [("A", a, pairs[0]), ("B", b, pairs[1])] {
        assert_eq!(on_hand_milli(db, art, loc).await, -5000, "{pc}: still below zero");
        let list = negative_stock::list_reviews(db, true).await.expect("list");
        assert!(list.double_counts.is_empty(), "{pc}: the double count is closed");
        assert_eq!(list.closed_double_counts.len(), 1, "{pc}: closed by the reversal");
        let open: Vec<_> = list.items.iter().filter(|i| i.status == "open").collect();
        assert_eq!(open.len(), 1, "{pc}: the location stays to reconcile");
        assert_eq!((open[0].article_id, open[0].location_id), (art, loc), "{pc}");
    }
}

// ── One count posted on two PCs (AU-100) ────────────────────────────────────────────────────

/// Submits a session from [`enter_count`], approves its variance and the count (not posted).
async fn approve_count(
    db: &DatabaseConnection,
    user: i32,
    (session, line): (
        crate::inventory::domain::InventoryCountSession,
        crate::inventory::domain::InventoryCountLine,
    ),
) -> crate::inventory::domain::InventoryCountSession {
    use crate::inventory::controls;
    let session = controls::transition_count_session(db, move_to(&session, "submitted", user))
        .await
        .expect("submitted");
    controls::approve_count_line(
        db,
        crate::inventory::domain::ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: i64::from(user),
            reviewer_evidence: "Bac vérifié au comptage".into(),
        },
    )
    .await
    .expect("approve line");
    controls::transition_count_session(db, move_to(&session, "approved", user))
        .await
        .expect("approved")
}

/// The count session of `db` with this sync id, as it stands now.
async fn session_by_sync_id(db: &DatabaseConnection, sync_id: &str) -> crate::inventory::domain::InventoryCountSession {
    let id = scalar_i64(
        db,
        "SELECT id FROM inventory_count_sessions WHERE entity_sync_id = ?",
        vec![sync_id.into()],
    )
    .await;
    crate::inventory::controls::list_count_sessions(db)
        .await
        .expect("sessions")
        .into_iter()
        .find(|s| s.id == id)
        .expect("session")
}

async fn text_of(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> String {
    crate::sync::test_support::scalar_opt_text(db, sql, values)
        .await
        .unwrap_or_else(|| panic!("{sql}: no text"))
}

/// `(movements, distinct sync ids)` of a movement type of a count session on `db`.
async fn count_movements(db: &DatabaseConnection, session_sync_id: &str, movement_type: &str) -> (i64, i64) {
    let sql = |what: &str| {
        format!(
            "SELECT {what} FROM inventory_transactions t JOIN inventory_count_sessions s ON s.id = t.source_id
             WHERE t.source_type = 'COUNT_SESSION' AND s.entity_sync_id = ? AND t.movement_type = ?"
        )
    };
    (
        scalar_i64(db, &sql("COUNT(*)"), vec![session_sync_id.into(), movement_type.into()]).await,
        scalar_i64(
            db,
            &sql("COUNT(DISTINCT t.entity_sync_id)"),
            vec![session_sync_id.into(), movement_type.into()],
        )
        .await,
    )
}

/// The sync id of the one movement of a type of a count session on `db`.
async fn movement_sync_id(db: &DatabaseConnection, session_sync_id: &str, movement_type: &str) -> String {
    text_of(
        db,
        "SELECT t.entity_sync_id FROM inventory_transactions t JOIN inventory_count_sessions s ON s.id = t.source_id
         WHERE t.source_type = 'COUNT_SESSION' AND s.entity_sync_id = ? AND t.movement_type = ?",
        vec![session_sync_id.into(), movement_type.into()],
    )
    .await
}

/// Audit links of a movement on `db`.
async fn audit_links(db: &DatabaseConnection, movement_sync_id: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM inventory_mutation_audit_links l JOIN inventory_transactions t ON t.id = l.transaction_id
         WHERE t.entity_sync_id = ?",
        vec![movement_sync_id.into()],
    )
    .await
}

/// One exchange where conflicts on the count session and its line are expected (the same record
/// changed on two PCs); nothing may fail to apply.
async fn sync_applied(server: &mut TestServer, device: &str, db: &DatabaseConnection) {
    let report = server.exchange(device, db).await;
    assert!(
        report.materialized.errors.is_empty(),
        "{device}: {:?}",
        report.materialized.errors
    );
}

/// `entity type:conflict type` of every sync conflict still open for a person on `db`, sorted.
async fn conflict_kinds(db: &DatabaseConnection) -> Vec<String> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT entity_type || ':' || conflict_type AS kind FROM sync_conflicts
         WHERE status IN ('new', 'triaged', 'escalated') ORDER BY kind"
            .to_string(),
    ))
    .await
    .expect("conflicts")
    .iter()
    .map(|r| r.try_get::<String>("", "kind").expect("kind"))
    .collect()
}

/// The row of `entity_type` with this sync id on `db`, as the wire carries it.
async fn wire_row(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
) -> serde_json::Map<String, serde_json::Value> {
    crate::sync::test_support::current_payload(db, entity_type, sync_id, &Default::default())
        .await
        .expect("payload")
        .expect("row")
}

/// Audit events "count posted twice" for a target on `db`.
async fn posted_twice_audits(db: &DatabaseConnection, target: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_id = ?",
        vec![crate::inventory::count_sync::AUDIT_POSTED_TWICE.into(), target.into()],
    )
    .await
}

/// One row of a count posted (or reversed) on both PCs, compared before they synchronise.
struct PostedRow {
    entity_type: &'static str,
    sync_id: String,
    /// B's version differs from A's (otherwise the server acknowledges B's push as a duplicate).
    differs: bool,
}

/// The movement, audit link (when any), session and line of a count posted on A and B, each with
/// whether B's version differs from A's. Taken after both postings, before any exchange.
async fn posted_rows(
    a: &DatabaseConnection,
    b: &DatabaseConnection,
    session: &str,
    movement: &str,
    link: Option<&str>,
) -> Vec<PostedRow> {
    let line = text_of(
        a,
        "SELECT cl.entity_sync_id FROM inventory_count_lines cl
         JOIN inventory_count_sessions s ON s.id = cl.session_id WHERE s.entity_sync_id = ?",
        vec![session.into()],
    )
    .await;
    let mut ids = vec![
        ("inventory_transactions", movement.to_string()),
        ("inventory_count_sessions", session.to_string()),
        ("inventory_count_lines", line),
    ];
    if let Some(link) = link {
        ids.push(("inventory_mutation_audit_links", link.to_string()));
    }
    let mut rows = Vec::new();
    for (entity_type, sync_id) in ids {
        let differs = wire_row(a, entity_type, &sync_id).await != wire_row(b, entity_type, &sync_id).await;
        rows.push(PostedRow {
            entity_type,
            sync_id,
            differs,
        });
    }
    rows
}

/// After the same count was posted (or reversed) on A first and B second and both synchronised
/// (then once more each, so B's audits reach A): B holds A's version of every row. A row that was
/// identical on both was acknowledged as a duplicate (no audit). A row that differed was settled:
/// B's push is closed (`superseded`, `COUNT_ALREADY_POSTED`) and B's own version is audited — for
/// the movement, the link and the session; a line names no poster — and the audit reaches A. No
/// conflict is left for a person on either PC.
async fn assert_first_posting_kept(
    server: &mut TestServer,
    a: &DatabaseConnection,
    b: &DatabaseConnection,
    rows: &[PostedRow],
) {
    sync_applied(server, "B", b).await;
    sync_applied(server, "A", a).await;
    for row in rows {
        let (entity_type, sync_id) = (row.entity_type, row.sync_id.as_str());
        assert_eq!(
            wire_row(b, entity_type, sync_id).await,
            wire_row(a, entity_type, sync_id).await,
            "B holds A's {entity_type}"
        );
        let audits = i64::from(row.differs && entity_type != "inventory_count_lines");
        assert_eq!(
            posted_twice_audits(b, sync_id).await,
            audits,
            "B audits its own {entity_type}"
        );
        assert_eq!(
            posted_twice_audits(a, sync_id).await,
            audits,
            "B's audit of {entity_type} reached A"
        );
        let expected = if row.differs {
            "superseded:COUNT_ALREADY_POSTED"
        } else {
            "acked:"
        };
        assert_eq!(
            crate::sync::test_support::scalar_opt_text(
                b,
                "SELECT status || ':' || COALESCE(rejection_code, '') FROM sync_outbox
                 WHERE entity_type = ? AND entity_sync_id = ? ORDER BY id DESC LIMIT 1",
                vec![entity_type.into(), sync_id.into()],
            )
            .await
            .as_deref(),
            Some(expected),
            "B's push of its {entity_type}"
        );
    }
    for (pc, db) in [("A", a), ("B", b)] {
        assert_eq!(
            conflict_kinds(db).await,
            Vec::<String>::new(),
            "{pc}: nothing left to a person"
        );
        let report = crate::audit::chain::verify_audit_chain(db).await.expect("verify");
        assert!(
            report.intact,
            "{pc}: every audit chain verifies (B's audits reached A): {report:?}"
        );
    }
}

/// Stock 10 known to A and B, and a count of 7 (variance −3) entered and approved on A, received by
/// B: the same approved session on both PCs.
struct ApprovedOnBoth {
    a: DatabaseConnection,
    b: DatabaseConnection,
    server: TestServer,
    /// The counting user of A and of B (local ids).
    users: (i32, i32),
    /// `(article, location)` local ids on A and on B.
    pair_a: (i64, i64),
    pair_b: (i64, i64),
    session_sync_id: String,
}

async fn approved_on_both() -> ApprovedOnBoth {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    let user_a = user_with(&a, "au100counta", &[crate::rbac::permissions::INV_MANAGE]).await;
    let user_b = user_with(&b, "au100countb", &[crate::rbac::permissions::INV_MANAGE]).await;
    let pair_a = (article.rowid, location.rowid);
    let entered = enter_count(&a, user_a, pair_a.0, pair_a.1, 7.0).await;
    let session = approve_count(&a, user_a, entered).await;
    let session_sync_id = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let pair_b = local_pair(&b, &article, &location).await;
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        10_000,
        "B holds the 10 units"
    );
    assert_eq!(
        session_by_sync_id(&b, &session_sync_id).await.status,
        "approved",
        "B holds the approved session"
    );
    ApprovedOnBoth {
        a,
        b,
        server,
        users: (user_a, user_b),
        pair_a,
        pair_b,
        session_sync_id,
    }
}

async fn post_on(db: &DatabaseConnection, user: i32, session_sync_id: &str) {
    let session = session_by_sync_id(db, session_sync_id).await;
    crate::inventory::controls::post_count_session(
        db,
        crate::inventory::domain::PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(i64::from(user)),
        },
    )
    .await
    .expect("posted");
}

async fn reverse_on(db: &DatabaseConnection, user: i32, session_sync_id: &str) {
    let session = session_by_sync_id(db, session_sync_id).await;
    crate::inventory::controls::reverse_count_session(
        db,
        crate::inventory::domain::ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Feuille de comptage erronée".into(),
            actor_id: Some(i64::from(user)),
        },
    )
    .await
    .expect("reversed");
}

/// AU-100: the same approved count posted on two PCs before either synchronised. Its movement has
/// the same seeded id on both (`count_adjust` of the session, article and location sync ids), so
/// each PC receives the other's as a replay: the stock moves once (10 → 7) on both, with one
/// movement and one audit link.
#[tokio::test]
async fn a_count_posted_on_two_pcs_moves_the_stock_once() {
    let ApprovedOnBoth {
        a,
        b,
        mut server,
        users,
        pair_a,
        pair_b,
        session_sync_id,
    } = approved_on_both().await;
    post_on(&a, users.0, &session_sync_id).await;
    post_on(&b, users.1, &session_sync_id).await;

    let ids = text_of(
        &a,
        "SELECT (SELECT entity_sync_id FROM articles WHERE id = ?) || ':' ||
                (SELECT entity_sync_id FROM stock_locations WHERE id = ?)",
        vec![pair_a.0.into(), pair_a.1.into()],
    )
    .await;
    let expected = crate::sync::identity::seeded_entity_sync_id("count_adjust", &format!("{session_sync_id}:{ids}"));
    let on_a = movement_sync_id(&a, &session_sync_id, "COUNT_ADJUST").await;
    let on_b = movement_sync_id(&b, &session_sync_id, "COUNT_ADJUST").await;
    assert_eq!(on_a, expected, "A: the seeded id");
    assert_eq!(on_b, expected, "B: the same id");
    // B posted at another time: every row differs from A's (the movement and the session also by
    // who posted them), so each is settled rather than acknowledged as a duplicate.
    let link = crate::inventory::controls::count_audit_link_sync_id(&expected);
    for sql in [
        "UPDATE inventory_transactions SET performed_at = '2026-10-01T10:00:00Z' WHERE entity_sync_id = ?",
        "UPDATE inventory_mutation_audit_links SET created_at = '2026-10-01T10:00:00Z' WHERE entity_sync_id = ?",
    ] {
        let id = if sql.contains("audit_links") { &link } else { &expected };
        exec(&b, sql, vec![id.clone().into()]).await;
    }
    exec(
        &b,
        "UPDATE inventory_count_sessions SET posted_at = '2026-10-01T10:00:00Z' WHERE entity_sync_id = ?",
        vec![session_sync_id.clone().into()],
    )
    .await;
    exec(
        &b,
        "UPDATE inventory_count_lines SET updated_at = '2026-10-01T10:00:00Z'
         WHERE session_id = (SELECT id FROM inventory_count_sessions WHERE entity_sync_id = ?)",
        vec![session_sync_id.clone().into()],
    )
    .await;
    let rows = posted_rows(&a, &b, &session_sync_id, &expected, Some(&link)).await;
    assert!(rows.iter().all(|r| r.differs), "every row differs");

    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    sync_applied(&mut server, "A", &a).await;
    for (pc, db, (art, loc)) in [("A", &a, pair_a), ("B", &b, pair_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, 7_000, "{pc}: corrected once");
        assert_eq!(
            count_movements(db, &session_sync_id, "COUNT_ADJUST").await,
            (1, 1),
            "{pc}: one movement"
        );
        assert_eq!(audit_links(db, &expected).await, 1, "{pc}: one audit link");
    }
    assert_first_posting_kept(&mut server, &a, &b, &rows).await;
}

/// AU-100: the same posted count reversed on two PCs before either synchronised: one
/// `count_reversal` movement with the same seeded id on both, so the stock goes back once (7 → 10).
#[tokio::test]
async fn a_count_reversed_on_two_pcs_moves_the_stock_back_once() {
    let ApprovedOnBoth {
        a,
        b,
        mut server,
        users,
        pair_a,
        pair_b,
        session_sync_id,
    } = approved_on_both().await;
    post_on(&a, users.0, &session_sync_id).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        7_000,
        "B: the posting arrived"
    );

    reverse_on(&a, users.0, &session_sync_id).await;
    reverse_on(&b, users.1, &session_sync_id).await;
    let on_a = movement_sync_id(&a, &session_sync_id, "COUNT_REVERSAL").await;
    assert_eq!(on_a, movement_sync_id(&b, &session_sync_id, "COUNT_REVERSAL").await);
    assert_ne!(
        on_a,
        movement_sync_id(&a, &session_sync_id, "COUNT_ADJUST").await,
        "a reversal has its own id"
    );
    // As it comes: the movement and the session name each PC's user (they differ); the line may be
    // identical on both (acknowledged as a duplicate) or not (settled) — both are checked.
    let rows = posted_rows(&a, &b, &session_sync_id, &on_a, None).await;

    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    sync_applied(&mut server, "A", &a).await;
    for (pc, db, (art, loc)) in [("A", &a, pair_a), ("B", &b, pair_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, 10_000, "{pc}: back once");
        assert_eq!(
            count_movements(db, &session_sync_id, "COUNT_REVERSAL").await,
            (1, 1),
            "{pc}: one reversal"
        );
        assert_eq!(
            count_movements(db, &session_sync_id, "COUNT_ADJUST").await,
            (1, 1),
            "{pc}: one posting"
        );
    }
    assert_first_posting_kept(&mut server, &a, &b, &rows).await;
}

/// AU-100, mixed versions: a count movement from a PC of an older version (random sync id) for a
/// count this PC already holds is applied like any movement — the ledger stays the same on every PC
/// — and the count is listed "comptage passé deux fois — à vérifier"; the same movement replayed
/// with its own id is a no-op; a movement of another session of the same article and location is
/// applied, and that session can then no longer be posted here (refused: its count already moved the
/// stock).
#[tokio::test]
async fn a_count_movement_received_under_another_id_is_applied_and_listed() {
    use crate::inventory::controls;
    use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let user = user_with(&db, "au100inbound", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let entered = enter_count(&db, user, art, loc, 7.0).await;
    let session = approve_count(&db, user, entered).await;
    let session_sync_id = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    post_on(&db, user, &session_sync_id).await;
    assert_eq!(on_hand_milli(&db, art, loc).await, 7_000);
    let held = movement_sync_id(&db, &session_sync_id, "COUNT_ADJUST").await;
    // The row as the wire carries it (references by sync id).
    let payload = crate::sync::test_support::current_payload(&db, "inventory_transactions", &held, &Default::default())
        .await
        .expect("payload")
        .expect("row");
    assert_eq!(
        payload.get("source_sync_id").and_then(|v| v.as_str()),
        Some(session_sync_id.as_str()),
        "the session travels as source_sync_id"
    );
    let json = serde_json::Value::Object(payload.clone()).to_string();

    // The same count posted on a PC of an older version: a random id.
    let random_id = crate::sync::identity::new_entity_sync_id();
    let outcome = apply_inbound_payload(&db, "inventory_transactions", &random_id, "upsert", 1, &json)
        .await
        .expect("apply");
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(on_hand_milli(&db, art, loc).await, 4_000, "applied as received");
    assert_eq!(count_movements(&db, &session_sync_id, "COUNT_ADJUST").await, (2, 2));
    let list = negative_stock::list_reviews(&db, false).await.expect("list");
    assert_eq!(list.double_postings.len(), 1, "listed as a count posted twice");

    // This version's movement received again (same id): a replay, no-op.
    let replay = apply_inbound_payload(&db, "inventory_transactions", &held, "upsert", 1, &json)
        .await
        .expect("replay");
    assert!(matches!(replay, ApplyOutcome::Unchanged), "{replay:?}");
    assert_eq!(on_hand_milli(&db, art, loc).await, 4_000);

    // Another session of the same article and location, approved here but not posted: a movement of
    // that session from another PC is not this count, so it is applied.
    let other = approve_count(&db, user, enter_count(&db, user, art, loc, 5.0).await).await;
    let other_sync_id = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![other.id.into()],
    )
    .await;
    let mut for_other = payload;
    for_other.insert("source_sync_id".into(), other_sync_id.clone().into());
    for_other.insert("quantity".into(), serde_json::json!(1.0));
    let applied = apply_inbound_payload(
        &db,
        "inventory_transactions",
        &crate::sync::identity::new_entity_sync_id(),
        "upsert",
        1,
        &serde_json::Value::Object(for_other).to_string(),
    )
    .await
    .expect("apply other");
    assert!(matches!(applied, ApplyOutcome::Applied), "{applied:?}");
    assert_eq!(
        on_hand_milli(&db, art, loc).await,
        5_000,
        "another count is applied (4 + 1)"
    );

    // That session's count already moved the stock here: posting it again is refused.
    let refused = controls::post_count_session(
        &db,
        crate::inventory::domain::PostInventoryCountSessionInput {
            session_id: other.id,
            expected_row_version: other.row_version,
            actor_id: Some(i64::from(user)),
        },
    )
    .await
    .expect_err("already posted on another PC");
    assert_eq!(refusal_code(refused), controls::COUNT_ALREADY_POSTED);
    assert_eq!(on_hand_milli(&db, art, loc).await, 5_000, "nothing moved twice");
    assert_eq!(count_movements(&db, &other_sync_id, "COUNT_ADJUST").await, (1, 1));
}

/// CMMS Advisor decision (a): a count already posted twice in a database (two `COUNT_ADJUST`
/// movements of one session, article and location, from versions that gave each PC's posting its
/// own id) is one "comptage passé deux fois — à vérifier" item, the same on every PC holding the
/// ledger, naming the session and both movements; a new count of that article and location (the
/// stock-to-reconcile resolution, reason of at least 8 characters, audited) closes it on every PC.
#[tokio::test]
async fn a_count_posted_twice_is_an_item_that_a_new_count_closes() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    let user = user_with(&a, "au100twice", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let session = approve_count(&a, user, enter_count(&a, user, art, loc, 7.0).await).await;
    let session_sync_id = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    post_on(&a, user, &session_sync_id).await;
    let first = movement_sync_id(&a, &session_sync_id, "COUNT_ADJUST").await;
    // The same posting made on a PC of an older version (its own random id) and applied to the
    // stock there, as such a database holds it.
    let second = crate::sync::identity::new_entity_sync_id();
    exec(
        &a,
        "INSERT INTO inventory_transactions
            (entity_sync_id, article_id, warehouse_id, location_id, movement_type, quantity, source_type,
             source_id, source_ref, reason, performed_by_id, performed_at, row_version)
         SELECT ?, article_id, warehouse_id, location_id, movement_type, quantity, source_type, source_id,
                source_ref, reason, performed_by_id, '2026-10-01T09:00:00Z', 1
         FROM inventory_transactions WHERE entity_sync_id = ?",
        vec![second.clone().into(), first.clone().into()],
    )
    .await;
    exec(
        &a,
        "UPDATE stock_balances SET on_hand_qty = on_hand_qty - 3, available_qty = available_qty - 3
         WHERE article_id = ? AND location_id = ?",
        vec![art.into(), loc.into()],
    )
    .await;
    assert_eq!(on_hand_milli(&a, art, loc).await, 4_000, "corrected twice");
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    let (art_b, loc_b) = local_pair(&b, &article, &location).await;
    assert_eq!(on_hand_milli(&b, art_b, loc_b).await, 4_000, "B: the same ledger");

    let mut ids = vec![first.clone(), second.clone()];
    ids.sort();
    for (pc, db, (art, loc)) in [("A", &a, (art, loc)), ("B", &b, (art_b, loc_b))] {
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        assert!(list.items.is_empty(), "{pc}: nothing below zero");
        assert!(list.double_counts.is_empty(), "{pc}: not a double count of two PCs");
        assert_eq!(list.double_postings.len(), 1, "{pc}: one item");
        let item = &list.double_postings[0];
        assert_eq!((item.article_id, item.location_id), (art, loc), "{pc}");
        assert_eq!(item.session_code, session.session_code, "{pc}: the count session");
        assert_eq!(item.movement_type, "COUNT_ADJUST", "{pc}");
        let mut named: Vec<String> = item.movements.iter().map(|m| m.movement_sync_id.clone()).collect();
        named.sort();
        assert_eq!(named, ids, "{pc}: both movements");
        assert!(
            item.movements
                .iter()
                .all(|m| (m.quantity + 3.0).abs() < 1e-9 && !m.posted_at.is_empty()),
            "{pc}: quantity and time of each movement"
        );
        assert!((item.variance_qty + 3.0).abs() < 1e-9, "{pc}: the count's correction");
        assert!((item.excess_qty + 3.0).abs() < 1e-9, "{pc}: moved once too often");
        assert_eq!(item.on_hand_qty, 4.0, "{pc}");
        assert!(item.closed_by.is_none(), "{pc}");
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").double_posting_count,
            1,
            "{pc}"
        );
    }

    // A reason under 8 characters is refused; a new count with its reason closes it.
    let short = negative_stock::resolve_by_count(&a, user, resolution(art, loc, 7.0, "court"))
        .await
        .expect_err("short reason");
    assert_eq!(refusal_code(short), REFUSED_REASON_SHORT);
    let resolved =
        negative_stock::resolve_by_count(&a, user, resolution(art, loc, 7.0, "Recompté après double passage"))
            .await
            .expect("a new count");
    assert!(
        (resolved.variance_qty - 3.0).abs() < 1e-9,
        "the stock is set to the count"
    );
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.negative_stock.counted'
             AND json_extract(details_json, '$.double_posting') = 1",
            vec![],
        )
        .await,
        1,
        "the count is audited as resolving it"
    );
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    for (pc, db, (art, loc)) in [("A", &a, (art, loc)), ("B", &b, (art_b, loc_b))] {
        assert_eq!(on_hand_milli(db, art, loc).await, 7_000, "{pc}: the counted stock");
        let open = negative_stock::list_reviews(db, false).await.expect("list");
        assert!(open.double_postings.is_empty(), "{pc}: closed");
        let all = negative_stock::list_reviews(db, true).await.expect("list");
        assert_eq!(all.closed_double_postings.len(), 1, "{pc}: kept in the closed history");
        assert_eq!(
            all.closed_double_postings[0]
                .closed_by
                .as_ref()
                .map(|c| c.session_code.clone()),
            Some(resolved.session_code.clone()),
            "{pc}: closed by the new count"
        );
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").double_posting_count,
            0,
            "{pc}"
        );
    }
}

/// A count posted twice is not closed by a count entered before the second movement was here: its
/// variance was set against a stock missing that movement.
#[tokio::test]
async fn a_count_posted_twice_stays_open_after_a_count_that_missed_a_movement() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let user = user_with(&db, "au100missed", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let session = approve_count(&db, user, enter_count(&db, user, art, loc, 7.0).await).await;
    let session_sync_id = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    post_on(&db, user, &session_sync_id).await;
    // A later count entered (and so marked) before the second movement was here.
    let later = enter_count(&db, user, art, loc, 7.0).await;
    let first = movement_sync_id(&db, &session_sync_id, "COUNT_ADJUST").await;
    exec(
        &db,
        "INSERT INTO inventory_transactions
            (entity_sync_id, article_id, warehouse_id, location_id, movement_type, quantity, source_type,
             source_id, source_ref, reason, performed_by_id, performed_at, row_version)
         SELECT ?, article_id, warehouse_id, location_id, movement_type, quantity, source_type, source_id,
                source_ref, reason, performed_by_id, performed_at, 1
         FROM inventory_transactions WHERE entity_sync_id = ?",
        vec![crate::sync::identity::new_entity_sync_id().into(), first.into()],
    )
    .await;
    post_count(&db, user, later).await;
    let list = negative_stock::list_reviews(&db, false).await.expect("list");
    assert_eq!(list.double_postings.len(), 1, "still to verify");
}

/// The same count posted on two PCs whose count lines really differ (here the reviewer's note was
/// changed on B before posting): the session, the movement and its audit link are settled as above,
/// but the line stays a conflict for a person; the stock still moved once.
#[tokio::test]
async fn a_count_posted_on_two_pcs_with_different_lines_leaves_the_line_to_a_person() {
    let ApprovedOnBoth {
        a,
        b,
        mut server,
        users,
        pair_a,
        pair_b,
        session_sync_id,
    } = approved_on_both().await;
    exec(
        &b,
        "UPDATE inventory_count_lines SET approval_note = 'Note différente saisie sur B'
         WHERE session_id = (SELECT id FROM inventory_count_sessions WHERE entity_sync_id = ?)",
        vec![session_sync_id.clone().into()],
    )
    .await;
    post_on(&a, users.0, &session_sync_id).await;
    post_on(&b, users.1, &session_sync_id).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    sync_applied(&mut server, "A", &a).await;
    for (pc, db, (art, loc)) in [("A", &a, pair_a), ("B", &b, pair_b)] {
        assert_eq!(on_hand_milli(db, art, loc).await, 7_000, "{pc}: corrected once");
    }
    assert_eq!(conflict_kinds(&a).await, Vec::<String>::new(), "A pushed first");
    let kinds = conflict_kinds(&b).await;
    assert!(!kinds.is_empty(), "B: the line is left to a person");
    assert!(
        kinds.iter().all(|k| k.starts_with("inventory_count_lines:")),
        "B: only the line: {kinds:?}"
    );
}

/// AU-100, review S3: the same session posted on two PCs whose lines counted different quantities
/// (7 on A, 6 on B: variances −3 and −4). The movement A pushed first is kept on both and B's stock
/// is corrected by the difference, so both hold the same ledger and the same stock (7); B's own
/// posting is audited (both quantities) and the count is listed "comptage divergent — à vérifier"
/// (on B, then on A once the audit arrives); the line stays a conflict for a person on B; a new
/// count closes the item.
#[tokio::test]
async fn a_count_posted_on_two_pcs_with_different_quantities_converges_and_is_listed() {
    let ApprovedOnBoth {
        a,
        b,
        mut server,
        users,
        pair_a,
        pair_b,
        session_sync_id,
    } = approved_on_both().await;
    exec(
        &b,
        "UPDATE inventory_count_lines SET counted_qty = 6, variance_qty = -4
         WHERE session_id = (SELECT id FROM inventory_count_sessions WHERE entity_sync_id = ?)",
        vec![session_sync_id.clone().into()],
    )
    .await;
    post_on(&a, users.0, &session_sync_id).await;
    post_on(&b, users.1, &session_sync_id).await;
    assert_eq!(on_hand_milli(&a, pair_a.0, pair_a.1).await, 7_000);
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 6_000);
    let movement = movement_sync_id(&a, &session_sync_id, "COUNT_ADJUST").await;

    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    sync_applied(&mut server, "A", &a).await;
    for (pc, db, (art, loc)) in [("A", &a, pair_a), ("B", &b, pair_b)] {
        assert_eq!(
            on_hand_milli(db, art, loc).await,
            7_000,
            "{pc}: the stock A posted first"
        );
        assert_eq!(
            count_movements(db, &session_sync_id, "COUNT_ADJUST").await,
            (1, 1),
            "{pc}: one movement"
        );
        assert_eq!(
            wire_row(db, "inventory_transactions", &movement)
                .await
                .get("quantity")
                .and_then(|q| q.as_f64()),
            Some(-3.0),
            "{pc}: the same movement content"
        );
        let list = negative_stock::list_reviews(db, false).await.expect("list");
        let divergent: Vec<_> = list
            .double_postings
            .iter()
            .filter(|d| d.kind == negative_stock::DOUBLE_POSTING_DIVERGENT)
            .collect();
        assert_eq!(divergent.len(), 1, "{pc}: one divergent count");
        let item = divergent[0];
        assert_eq!((item.article_id, item.location_id), (art, loc), "{pc}");
        let quantities: Vec<f64> = item.movements.iter().map(|m| m.quantity).collect();
        assert_eq!(quantities, vec![-3.0, -4.0], "{pc}: the kept version, then B's");
        assert!(item.movements[1].device_code.is_some(), "{pc}: B's PC is named");
        assert!((item.excess_qty + 1.0).abs() < 1e-9, "{pc}: B's version differed by -1");
    }
    assert_eq!(posted_twice_audits(&b, &movement).await, 1, "B's posting is audited");
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_id = ?
             AND json_extract(details_json, '$.this_pc_posting.quantity') = -4
             AND json_extract(details_json, '$.kept_posting.quantity') = -3",
            vec![
                crate::inventory::count_sync::AUDIT_POSTED_TWICE.into(),
                movement.clone().into()
            ],
        )
        .await,
        1,
        "both quantities are in the audit"
    );
    assert_eq!(conflict_kinds(&a).await, Vec::<String>::new(), "A pushed first");
    let kinds = conflict_kinds(&b).await;
    assert!(!kinds.is_empty(), "B: the line is left to a person");
    assert!(
        kinds.iter().all(|k| k.starts_with("inventory_count_lines:")),
        "B: only the line: {kinds:?}"
    );

    // A new count on B (reason of at least 8 characters) closes it.
    let counter_b = user_with(&b, "au100divergent", &[crate::rbac::permissions::INV_MANAGE]).await;
    negative_stock::resolve_by_count(
        &b,
        counter_b,
        resolution(pair_b.0, pair_b.1, 7.0, "Recompté après comptage divergent"),
    )
    .await
    .expect("a new count");
    let list = negative_stock::list_reviews(&b, false).await.expect("list");
    assert!(list.double_postings.is_empty(), "B: closed by the new count");
}

/// Audit events written by the stock code, and how they leave this PC (reviews 8e1d2ffd B1,
/// bdc259be B1/S1):
/// - a "count posted twice" event written now (targets `inventory_transactions`,
///   `inventory_mutation_audit_links`, `inventory_count_sessions`: sync ids, sent as they are) is
///   sealed over its sync payload and travels;
/// - a "stock to reconcile" event (`stock_location_article`: local ids) is kept on this PC, sealed in
///   its local form with a stub;
/// - an event an earlier version already kept on this PC (sealed `L2:` with its stub, its capture
///   journal row still waiting) never starts travelling, although this version could encode it.
///
/// Nothing is left to a person on A, and both PCs verify A's chain without a break.
#[tokio::test]
async fn the_stock_audit_events_travel_or_stay_as_they_were_sealed() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let write = |target_type: &'static str, target_id: String| {
        let a = &a;
        async move {
            let id = crate::audit::writer::write_audit_event(
                a,
                crate::audit::writer::AuditEventInput {
                    action_code: "inventory.test.audit_target".to_string(),
                    target_type: Some(target_type.to_string()),
                    target_id: Some(target_id),
                    actor_id: None,
                    auth_context: "system".to_string(),
                    result: "success".to_string(),
                    before_hash: None,
                    after_hash: None,
                    retention_class: "standard".to_string(),
                    details_json: Some(serde_json::json!({ "summary": "test" })),
                },
            )
            .await
            .expect("audit");
            text_of(
                a,
                "SELECT entity_sync_id FROM audit_events WHERE id = ?",
                vec![id.into()],
            )
            .await
        }
    };
    // Kept on this PC by an earlier version (the last position of the chain when resealed).
    let kept_before = write("inventory_transactions", crate::sync::identity::new_entity_sync_id()).await;
    crate::audit::chain::reseal_kept_on_device(&a, &kept_before)
        .await
        .expect("reseal");
    let stock_to_reconcile = write("stock_location_article", "12:34".to_string()).await;
    let mut travelling = Vec::new();
    for target_type in [
        "inventory_transactions",
        "inventory_mutation_audit_links",
        "inventory_count_sessions",
    ] {
        let target = crate::sync::identity::new_entity_sync_id();
        travelling.push((target_type, target.clone(), write(target_type, target).await));
    }

    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    assert_eq!(
        conflict_kinds(&a).await,
        Vec::<String>::new(),
        "A: nothing failed to stage"
    );
    let on_b = |sync_id: String| {
        let b = &b;
        async move {
            scalar_i64(
                b,
                "SELECT COUNT(*) FROM audit_events WHERE entity_sync_id = ?",
                vec![sync_id.into()],
            )
            .await
        }
    };
    assert_eq!(on_b(kept_before.clone()).await, 0, "an event kept before stays kept");
    assert_eq!(on_b(stock_to_reconcile.clone()).await, 0, "local ids stay on their PC");
    for sync_id in [&kept_before, &stock_to_reconcile] {
        let hash = text_of(
            &a,
            "SELECT chain_hash FROM audit_events WHERE entity_sync_id = ?",
            vec![sync_id.clone().into()],
        )
        .await;
        assert!(hash.starts_with("L2:"), "kept, sealed in its local form: {hash}");
    }
    for (target_type, target, sync_id) in &travelling {
        let received = crate::sync::test_support::scalar_opt_text(
            &b,
            "SELECT target_id FROM audit_events WHERE entity_sync_id = ? AND target_type = ?",
            vec![sync_id.clone().into(), (*target_type).into()],
        )
        .await;
        assert_eq!(
            received.as_deref(),
            Some(target.as_str()),
            "B received the {target_type} event"
        );
    }
    for (pc, db) in [("A", &a), ("B", &b)] {
        let report = crate::audit::chain::verify_audit_chain(db).await.expect("verify");
        assert!(report.intact, "{pc}: A's chain verifies without a break: {report:?}");
    }
}

/// Review S1: a "count posted twice" audit row that this version cannot read — details that are not
/// JSON, a detail shape without a quantity, or one naming a movement that is not a count — never
/// breaks the stock-to-reconcile list, its summary, nor recording and posting a count: it is left
/// out.
#[tokio::test]
async fn an_unreadable_posted_twice_audit_never_breaks_the_list_or_a_count() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let user = user_with(&db, "au100badaudit", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let session = approve_count(&db, user, enter_count(&db, user, art, loc, 7.0).await).await;
    let session_sync_id = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    post_on(&db, user, &session_sync_id).await;
    let count_movement = movement_sync_id(&db, &session_sync_id, "COUNT_ADJUST").await;
    let entry_movement = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_transactions
         WHERE article_id = ? AND location_id = ? AND source_type <> 'COUNT_SESSION' LIMIT 1",
        vec![art.into(), loc.into()],
    )
    .await;
    let write = |target: String, details: serde_json::Value| {
        let db = &db;
        async move {
            crate::audit::writer::write_audit_event(
                db,
                crate::audit::writer::AuditEventInput {
                    action_code: crate::inventory::count_sync::AUDIT_POSTED_TWICE.to_string(),
                    target_type: Some("inventory_transactions".to_string()),
                    target_id: Some(target),
                    actor_id: None,
                    auth_context: "system".to_string(),
                    result: "success".to_string(),
                    before_hash: None,
                    after_hash: None,
                    retention_class: "standard".to_string(),
                    details_json: Some(details),
                },
            )
            .await
            .expect("audit")
        }
    };
    // No quantity in the shape this version reads.
    write(count_movement.clone(), serde_json::json!({ "quantity_differs": true })).await;
    // A movement that is not a count.
    write(
        entry_movement,
        serde_json::json!({ "quantity_differs": true, "this_pc_posting": { "quantity": 1.0 } }),
    )
    .await;
    // Details that are not JSON at all.
    let broken = write(count_movement, serde_json::json!({ "quantity_differs": true })).await;
    exec(
        &db,
        "UPDATE audit_events SET details_json = '{not json' WHERE id = ?",
        vec![broken.into()],
    )
    .await;

    let list = negative_stock::list_reviews(&db, true)
        .await
        .expect("the list still loads");
    assert!(list.double_postings.is_empty(), "nothing listed from unreadable audits");
    assert!(list.closed_double_postings.is_empty());
    assert_eq!(
        negative_stock::summary(&db)
            .await
            .expect("summary")
            .double_posting_count,
        0
    );
    // Counting the location still works (its mark reads the same audits).
    let again = enter_count(&db, user, art, loc, 7.0).await;
    post_count(&db, user, again).await;
}
