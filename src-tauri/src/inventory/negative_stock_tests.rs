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
pub(super) async fn stocked_pair(
    a: &DatabaseConnection,
    factory: &mut RowFactory,
    qty: f64,
) -> (CreatedRow, CreatedRow) {
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
pub(super) async fn local_pair(db: &DatabaseConnection, article: &CreatedRow, location: &CreatedRow) -> (i64, i64) {
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

pub(super) async fn on_hand_milli(db: &DatabaseConnection, article_id: i64, location_id: i64) -> i64 {
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

/// The plant administrator who gives roles on `db`: the first administrator (bootstrap flag) created
/// here, or one received from another PC. Every grant names its author, as the screens record it,
/// so that other PCs accept it (`sync::grant_check`). On a second PC, sync with the first before
/// giving roles there: a PC that has not received the author yet creates its own, and a second
/// account with the bootstrap flag is refused once the plant has its first administrator.
async fn grant_author(db: &DatabaseConnection) -> i64 {
    let held = scalar_i64(
        db,
        "SELECT COALESCE((SELECT id FROM user_accounts WHERE username LIKE 'grant-author-%' AND is_admin = 1
                            AND deleted_at IS NULL ORDER BY id LIMIT 1), 0)",
        vec![],
    )
    .await;
    if held != 0 {
        return held;
    }
    let sync_id = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin,
            force_password_change, failed_login_attempts, created_at, updated_at, row_version)
         VALUES (?, ?, 'Grant author', 'local', 1, 1, 0, 0, datetime('now'), datetime('now'), 1)",
        vec![sync_id.clone().into(), format!("grant-author-{}", &sync_id[..8]).into()],
    )
    .await;
    scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
        vec![sync_id.into()],
    )
    .await
}

/// A plant-wide role holding `permissions`, given to a new account on `db` by the plant
/// administrator ([`grant_author`]).
pub(super) async fn user_with(db: &DatabaseConnection, name: &str, permissions: &[&str]) -> i32 {
    let author = grant_author(db).await;
    let user = crate::auth::scope_tests::user(db, name).await;
    let role_sync = format!("negstock-role-{name}");
    exec(
        db,
        "INSERT INTO roles (entity_sync_id, name, description, role_type, status, is_system, created_at, updated_at,
            row_version)
         VALUES (?, ?, NULL, 'custom', 'active', 0, datetime('now'), datetime('now'), 1)",
        vec![role_sync.clone().into(), format!("{name}-role").into()],
    )
    .await;
    let role = scalar_i64(
        db,
        "SELECT id FROM roles WHERE entity_sync_id = ?",
        vec![role_sync.into()],
    )
    .await;
    for permission in permissions {
        exec(
            db,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at, granted_by_id)
             SELECT ?, id, datetime('now'), ? FROM permissions WHERE name = ?",
            vec![role.into(), author.into(), (*permission).into()],
        )
        .await;
    }
    exec(
        db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
            assigned_by_id, created_at, updated_at, row_version)
         VALUES (?, ?, ?, 'tenant', NULL, ?, datetime('now'), datetime('now'), 1)",
        vec![
            format!("negstock-usa-{name}").into(),
            user.into(),
            role.into(),
            author.into(),
        ],
    )
    .await;
    user
}

/// Someone other than `user` who approves a count variance on `db` (four-eyes above the
/// tolerance, D11; the tests' tolerance is blank, so every variance is above it): the seeded
/// administrator account, the same on every PC (a test account made on one PC only would take
/// local ids another PC also uses).
pub(super) async fn second_person(db: &DatabaseConnection, user: i32) -> i32 {
    let admin = scalar_i64(
        db,
        "SELECT id FROM user_accounts WHERE username = 'admin' AND id <> ?",
        vec![user.into()],
    )
    .await;
    i32::try_from(admin).expect("id")
}

/// [`negative_stock::resolve_by_count`] by `user`, then — when the count waits for a second person
/// (above the four-eyes tolerance, D11 Q4a; at a location below zero every variance is) — its
/// approval by [`second_person`].
pub(super) async fn resolve_approved(
    db: &DatabaseConnection,
    user: i32,
    input: ResolveNegativeStockInput,
) -> Result<negative_stock::NegativeStockResolution, AppError> {
    let resolution = negative_stock::resolve_by_count(db, user, input).await?;
    if !resolution.pending_approval {
        return Ok(resolution);
    }
    let pending = negative_stock::list_reviews(db, false)
        .await?
        .pending_counts
        .into_iter()
        .find(|p| p.session_code == resolution.session_code)
        .expect("the count waits for its approval");
    let approver = second_person(db, user).await;
    negative_stock::approve_pending_count(
        db,
        approver,
        negative_stock::ApprovePendingCountInput {
            line_id: pending.line_id,
            expected_row_version: pending.row_version,
            reason: "Comptage vérifié par un second".into(),
        },
    )
    .await
}

pub(super) fn resolution(article_id: i64, location_id: i64, counted: f64, reason: &str) -> ResolveNegativeStockInput {
    ResolveNegativeStockInput {
        article_id,
        location_id,
        counted_qty: counted,
        variance_reason_code: "COUNT_ERROR".into(),
        reason: reason.into(),
    }
}

pub(super) fn refusal_code(err: AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

/// A second location of the same article with `qty` in stock, created on `a`.
pub(super) async fn second_location(
    a: &DatabaseConnection,
    factory: &mut RowFactory,
    article_id: i64,
    qty: f64,
) -> CreatedRow {
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
pub(super) async fn enter_count(
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
            expected_row_version: None,
            actor_id: Some(i64::from(user)),
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
pub(super) async fn post_count(
    db: &DatabaseConnection,
    user: i32,
    counted: (
        crate::inventory::domain::InventoryCountSession,
        crate::inventory::domain::InventoryCountLine,
    ),
) -> crate::inventory::domain::InventoryCountSession {
    let approver = second_person(db, user).await;
    post_count_approved_by(db, user, approver, counted).await
}

/// [`post_count`], the line's variance approved by `approver`.
pub(super) async fn post_count_approved_by(
    db: &DatabaseConnection,
    user: i32,
    approver: i32,
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
                reviewer_id: i64::from(approver),
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

    let resolved = resolve_approved(&a, manager, resolution(art_a, loc_a, 0.0, "Recompté : bac vide"))
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
               AND cl.counted_qty = 0 AND cl.variance_qty = 5 AND cl.approval_note = 'Comptage vérifié par un second'
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
    resolve_approved(
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

    resolve_approved(a, manager_a, resolution(art_a, loc_a, 0.0, "Comptage poste A"))
        .await
        .expect("A1 at −5");
    let cycle = enter_count(a, manager_a, art_a, loc_a, 0.0).await;
    assert_eq!((cycle.1.system_qty, cycle.1.variance_qty), (0.0, 0.0), "A2 at 0");
    post_count(a, manager_a, cycle).await;
    resolve_approved(c, manager_c, resolution(art_c, loc_c, 0.0, "Comptage poste C"))
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

    resolve_approved(a, manager_a, resolution(art_a, loc_a, 0.0, "Recomptage commun"))
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

    resolve_approved(&a, manager_a, resolution(art, loc, 0.0, "Comptage depuis la liste"))
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
    resolve_approved(&a, manager_a, resolution(art, loc, 0.0, "Comptage poste A"))
        .await
        .expect("A resolves");
    resolve_approved(&b, manager_b, resolution(art_b, loc_b, 0.0, "Comptage poste B"))
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

    let short = resolve_approved(&a, manager_a, resolution(art, loc, 0.0, "Recompt"))
        .await
        .expect_err("7 characters");
    assert_eq!(refusal_code(short), REFUSED_REASON_SHORT);
    let closed = resolve_approved(&a, manager_a, resolution(art, loc, 0.0, "Recomptage commun"))
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
    let again = resolve_approved(&a, manager_a, resolution(art, loc, 0.0, "Recomptage commun"))
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

    let denied = resolve_approved(&a, viewer, resolution(art, loc, 0.0, "Recompté bac"))
        .await
        .expect_err("inv.view only");
    assert!(matches!(denied, AppError::PermissionDenied(_)), "{denied:?}");
    for short in ["", "        ", "abcdefg", "  abcd  "] {
        let err = resolve_approved(&a, manager, resolution(art, loc, 0.0, short))
            .await
            .expect_err("short reason");
        assert_eq!(refusal_code(err), REFUSED_REASON_SHORT, "{short:?}");
    }
    assert_eq!(review_rows(&a, art, loc).await, 1);
    assert_eq!(on_hand_milli(&a, art, loc).await, -4000, "refusals write nothing");

    // Counted 0 while 1 is still reserved: the item closes, the reservation shows as a warning.
    let resolved = resolve_approved(&a, manager, resolution(art, loc, 0.0, " abcdefgh "))
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
    let again = resolve_approved(&a, manager, resolution(art, loc, 2.0, "Recompté bac"))
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
pub(super) async fn session_by_code(
    db: &DatabaseConnection,
    code: &str,
) -> crate::inventory::domain::InventoryCountSession {
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
    resolve_approved(
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
    resolve_approved(
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
            expected_row_version: Some(first.row_version),
            actor_id: Some(i64::from(counter_b)),
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
    resolve_approved(b, counter_b, resolution(art, loc, 0.0, "Comptage depuis la liste"))
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
            reviewer_id: i64::from(second_person(b, counter_b).await),
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
    let resolved_a = resolve_approved(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A1");
    resolve_approved(
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

    resolve_approved(a, manager, resolution(art, loc, 0.0, "Comptage poste A"))
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
    let resolved_a = resolve_approved(
        a,
        manager_a,
        resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage poste A"),
    )
    .await
    .expect("A1");
    resolve_approved(
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
            reviewer_id: i64::from(second_person(db, user).await),
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
    // that session from another PC is not this count, so it is applied — with the version of its
    // line naming it (four-eyes, CMMS Advisor Q8): alone, it waits for that version.
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
    let other_movement = crate::sync::identity::new_entity_sync_id();
    let other_json = serde_json::Value::Object(for_other).to_string();
    let apply_other =
        || apply_inbound_payload(&db, "inventory_transactions", &other_movement, "upsert", 1, &other_json);
    let waits = apply_other().await.expect("apply other alone");
    assert!(matches!(waits, ApplyOutcome::Deferred { .. }), "{waits:?}");
    let other_line = text_of(
        &db,
        "SELECT cl.entity_sync_id FROM inventory_count_lines cl WHERE cl.session_id = ?",
        vec![other.id.into()],
    )
    .await;
    let mut naming = wire_row(&db, "inventory_count_lines", &other_line).await;
    naming.insert("posted_transaction_sync_id".into(), other_movement.clone().into());
    exec(
        &db,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                                 row_version, payload_json, payload_hash, apply_status)
         VALUES ('b-other-pc', 'c1', 'inventory_count_lines', ?, 'upsert', 99, ?, 'h-other-line', 'pending')",
        vec![other_line.into(), serde_json::Value::Object(naming).to_string().into()],
    )
    .await;
    let applied = apply_other().await.expect("apply other");
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
    let short = resolve_approved(&a, user, resolution(art, loc, 7.0, "court"))
        .await
        .expect_err("short reason");
    assert_eq!(refusal_code(short), REFUSED_REASON_SHORT);
    let resolved = resolve_approved(&a, user, resolution(art, loc, 7.0, "Recompté après double passage"))
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
            all.closed_double_postings[0]
                .closed_by
                .as_ref()
                .map(|c| c.self_approved),
            Some(false),
            "{pc}: approved by a second person"
        );
        assert_eq!(
            negative_stock::summary(db).await.expect("summary").double_posting_count,
            0,
            "{pc}"
        );
    }
    // CMMS Advisor Q13: a count approved by its own counter is listed as an auto-approbation,
    // derived on read, although nothing stored says so (here a line as another PC's version would
    // hold it: approver = counter, not flagged).
    exec_unsent(
        &a,
        "UPDATE inventory_count_lines SET approved_by_id = counted_by_id, self_approved = 0
         WHERE session_id = (SELECT id FROM inventory_count_sessions WHERE session_code = ?)",
        vec![resolved.session_code.clone().into()],
    )
    .await;
    let all = negative_stock::list_reviews(&a, true).await.expect("list");
    let closed_by = all.closed_double_postings[0].closed_by.as_ref().expect("closed by");
    assert!(closed_by.self_approved, "shown as an auto-approbation");
    assert_eq!(closed_by.approved_by, closed_by.counted_by);
    assert_eq!(
        scalar_i64(
            &a,
            "SELECT COUNT(*) FROM inventory_count_lines cl JOIN inventory_count_sessions s ON s.id = cl.session_id
             WHERE s.session_code = ? AND cl.self_approved = 0",
            vec![resolved.session_code.clone().into()],
        )
        .await,
        1,
        "nothing stored is rewritten"
    );
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
    resolve_approved(
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

/// Review N1 (8e1d2ffd): the same session posted on two PCs with different counted quantities
/// (7 on A, 6 on B), then reversed on B while B's line conflict is still open (B's line still says
/// −4, the movement kept on every PC says −3): the reversal undoes the movement as the ledger holds
/// it (+3), so both PCs go back to 10, with one reversal of the same content.
#[tokio::test]
async fn a_divergent_count_reversed_on_the_pc_that_lost_goes_back_once() {
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
    for (pc, db) in [("A", &a), ("B", &b), ("A", &a), ("B", &b), ("A", &a)] {
        sync_applied(&mut server, pc, db).await;
    }
    for (pc, db, (art, loc)) in [("A", &a, pair_a), ("B", &b, pair_b)] {
        assert_eq!(
            on_hand_milli(db, art, loc).await,
            7_000,
            "{pc}: converged on A's posting"
        );
    }
    assert!(
        !conflict_kinds(&b).await.is_empty(),
        "B's line conflict is still open when B reverses"
    );

    reverse_on(&b, users.1, &session_sync_id).await;
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        10_000,
        "B: the movement is undone as the ledger holds it (+3), not by B's line (+4)"
    );
    let reversal = movement_sync_id(&b, &session_sync_id, "COUNT_REVERSAL").await;
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
            wire_row(db, "inventory_transactions", &reversal)
                .await
                .get("quantity")
                .and_then(|q| q.as_f64()),
            Some(3.0),
            "{pc}: the reversal undoes the kept movement"
        );
    }
}

// ── Four-eyes on a count from the list (D11 Q4a) ─────────────────────────────────────────────

fn gate(err: AppError) -> String {
    match err {
        AppError::ValidationFailed(messages) => messages[0].split(':').next().unwrap_or_default().to_string(),
        other => refusal_code(other),
    }
}

/// D11 Q4a: a count from the list above the four-eyes tolerance (at a location below zero every
/// variance is) is saved as counted and moves nothing; the item stays open and shows it waiting.
/// Its counter cannot approve it; a second person does, with a note of 8 characters at least, and
/// that posts the movement and closes the item in one step.
#[tokio::test]
async fn a_list_count_above_the_tolerance_waits_for_a_second_person() {
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyeslistcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let approver = user_with(db, "foureyeslistapprover", &[crate::rbac::permissions::INV_MANAGE]).await;

    let counted = negative_stock::resolve_by_count(db, counter, resolution(art, loc, 2.0, "Comptage du bac"))
        .await
        .expect("counted");
    assert!(counted.pending_approval && !counted.self_approved);
    assert_eq!(on_hand_milli(db, art, loc).await, -5000, "nothing moved");
    let list = negative_stock::list_reviews(db, false).await.expect("list");
    assert_eq!(list.items.len(), 1, "the item stays open");
    let pending = list.pending_counts.first().expect("waiting for approval").clone();
    assert_eq!(
        (pending.article_id, pending.location_id, pending.counted_by_id),
        (art, loc, Some(i64::from(counter)))
    );
    assert!((pending.variance_qty - 7.0).abs() < 1e-9);

    let approve = |user: i32, reason: &str| {
        negative_stock::approve_pending_count(
            db,
            user,
            negative_stock::ApprovePendingCountInput {
                line_id: pending.line_id,
                expected_row_version: pending.row_version,
                reason: reason.into(),
            },
        )
    };
    assert_eq!(
        gate(
            approve(counter, "Je confirme mon comptage")
                .await
                .expect_err("the counter")
        ),
        crate::inventory::count_four_eyes::REFUSED_SELF_APPROVAL
    );
    assert_eq!(
        refusal_code(approve(approver, " court ").await.expect_err("short")),
        REFUSED_REASON_SHORT
    );
    let done = approve(approver, "Recompté avec le magasinier")
        .await
        .expect("approved");
    assert!(!done.pending_approval);
    assert_eq!(
        on_hand_milli(db, art, loc).await,
        2000,
        "posted at the counted quantity"
    );
    assert_eq!(done.review.expect("item").status, "closed");
    let list = negative_stock::list_reviews(db, false).await.expect("list");
    assert!(list.items.is_empty() && list.pending_counts.is_empty());
    assert_eq!(
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM inventory_count_lines WHERE id = ? AND approved_by_id = ? AND counted_by_id = ?
               AND self_approved = 0 AND posted_transaction_id IS NOT NULL",
            vec![pending.line_id.into(), approver.into(), counter.into()],
        )
        .await,
        1
    );
    assert_eq!(
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'inventory.negative_stock.count_approved' AND actor_id = ?",
            vec![approver.into()],
        )
        .await,
        1
    );
    assert_eq!(
        refusal_code(
            approve(approver, "Recompté avec le magasinier")
                .await
                .expect_err("twice")
        ),
        negative_stock::REFUSED_COUNT_NOT_PENDING
    );
}

/// D11 Q4a: the stock on record moved after the count waiting for approval: approving it would no
/// longer bring the stock to what was counted — refused, the location is counted again; a new count
/// of the pair replaces the waiting one.
#[tokio::test]
async fn a_waiting_list_count_is_refused_once_the_stock_moved_and_replaced_by_a_new_count() {
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyesstalecounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let approver = user_with(db, "foureyesstaleapprover", &[crate::rbac::permissions::INV_MANAGE]).await;
    negative_stock::resolve_by_count(db, counter, resolution(art, loc, 1.0, "Comptage du bac"))
        .await
        .expect("counted");
    let first = negative_stock::list_reviews(db, false)
        .await
        .expect("list")
        .pending_counts[0]
        .clone();
    adjust_noted(db, art, loc, 1.0, "Retour trouvé au comptage")
        .await
        .expect("entry");
    let stale = negative_stock::approve_pending_count(
        db,
        approver,
        negative_stock::ApprovePendingCountInput {
            line_id: first.line_id,
            expected_row_version: first.row_version,
            reason: "Recompté avec le magasinier".into(),
        },
    )
    .await
    .expect_err("the stock moved");
    assert_eq!(refusal_code(stale), negative_stock::REFUSED_COUNT_STALE);
    assert_eq!(on_hand_milli(db, art, loc).await, -4000);

    negative_stock::resolve_by_count(db, counter, resolution(art, loc, 1.0, "Second comptage du bac"))
        .await
        .expect("counted again");
    let list = negative_stock::list_reviews(db, false).await.expect("list");
    assert_eq!(list.pending_counts.len(), 1, "the new count replaces the waiting one");
    assert_ne!(list.pending_counts[0].line_id, first.line_id);
    assert_eq!(
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM inventory_count_sessions WHERE session_code = ? AND status = 'cancelled'",
            vec![first.session_code.clone().into()],
        )
        .await,
        1
    );
}

/// D11 Q4a: under the four-eyes opt-out of the organisation's only site, a count from the list above
/// the tolerance posts at once, flagged "auto-approbation".
#[tokio::test]
async fn under_the_sites_opt_out_a_list_count_posts_at_once_flagged() {
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    crate::inventory::tests::single_site_opting_out(db, &mut RowFactory::for_device(7), true).await;
    let counter = user_with(db, "foureyesoptoutcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let done = negative_stock::resolve_by_count(db, counter, resolution(art, loc, 0.0, "Comptage du bac"))
        .await
        .expect("posted");
    assert!(!done.pending_approval && done.self_approved);
    assert_eq!(on_hand_milli(db, art, loc).await, 0);
    assert_eq!(
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM inventory_count_lines WHERE count_path = 'reconciliation' AND self_approved = 1
               AND approved_by_id = ? AND counted_by_id = ?",
            vec![counter.into(), counter.into()],
        )
        .await,
        1
    );
}

/// D11: a count line received from another PC approved by its counter above the tolerance is a
/// conflict unless flagged and the site opted out here; a flag on a line approved by a second
/// person is a conflict; a line approved by a second person applies.
#[tokio::test]
async fn a_received_count_line_approved_by_its_counter_is_judged_by_four_eyes() {
    use crate::sync::materialize::ApplyOutcome;
    use sea_orm::TransactionTrait;
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyessynccounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let other = user_with(db, "foureyessyncother", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, line) = enter_count(db, counter, art, loc, 2.0).await;
    assert_eq!(line.is_critical, 1);
    let line_sync = crate::sync::test_support::scalar_opt_text(
        db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![line.id.into()],
    )
    .await
    .expect("line sync id");
    let user_sync = |id: i32| {
        crate::sync::test_support::scalar_opt_text(
            db,
            "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
            vec![id.into()],
        )
    };
    let (counter_sync, other_sync) = (
        user_sync(counter).await.expect("counter"),
        user_sync(other).await.expect("other"),
    );
    let held = crate::permit::sync_check::local_wire(db, "inventory_count_lines", &line_sync)
        .await
        .expect("wire")
        .expect("line");
    let approved = |by: &str, flag: i64| {
        let mut p = held.clone();
        p.insert("approved_by_sync_id".into(), serde_json::json!(by));
        p.insert("approved_at".into(), serde_json::json!("2026-10-23T10:00:00Z"));
        p.insert("approval_note".into(), serde_json::json!("Recompté avec le magasinier"));
        p.insert("self_approved".into(), serde_json::json!(flag));
        p.insert("row_version".into(), serde_json::json!(line.row_version + 1));
        p
    };
    let apply = |payload: serde_json::Map<String, serde_json::Value>| {
        let line_sync = line_sync.clone();
        async move {
            let layout = crate::sync::row_layout::layout(db, "inventory_count_lines")
                .await
                .expect("layout");
            let tx = db.begin().await.expect("begin");
            let outcome = crate::sync::row_codec::apply_row(
                &tx,
                &layout,
                &line_sync,
                "upsert",
                &serde_json::to_string(&payload).expect("json"),
            )
            .await
            .expect("apply");
            tx.rollback().await.expect("rollback");
            outcome
        }
    };
    let code = |outcome: &ApplyOutcome| match outcome {
        ApplyOutcome::Conflict { code, .. } => code.clone(),
        other => format!("{other:?}"),
    };
    for flag in [0, 1] {
        let outcome = apply(approved(&counter_sync, flag)).await;
        assert_eq!(code(&outcome), "COUNT_SELF_APPROVAL", "flag {flag}: {outcome:?}");
    }
    // No planning site applied here (the organisation still arriving): a self-approval waits.
    exec(
        db,
        "UPDATE org_nodes SET deleted_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE deleted_at IS NULL",
        vec![],
    )
    .await;
    assert!(
        crate::inventory::count_four_eyes::store_rule(db)
            .await
            .expect("rule")
            .no_site
    );
    let outcome = apply(approved(&counter_sync, 1)).await;
    assert!(
        matches!(outcome, ApplyOutcome::Deferred { .. }),
        "no site here yet: waits, {outcome:?}"
    );
    let outcome = apply(approved(&counter_sync, 0)).await;
    assert_eq!(
        code(&outcome),
        "COUNT_SELF_APPROVAL",
        "not flagged, no site: {outcome:?}"
    );
    let outcome = apply(approved(&other_sync, 1)).await;
    assert_eq!(code(&outcome), "COUNT_APPROVAL_RECORD_INVALID", "{outcome:?}");
    let outcome = apply(approved(&other_sync, 0)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    crate::inventory::tests::single_site_opting_out(db, &mut RowFactory::for_device(7), true).await;
    let outcome = apply(approved(&counter_sync, 1)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "opted out here: {outcome:?}");
    let outcome = apply(approved(&counter_sync, 0)).await;
    assert_eq!(code(&outcome), "COUNT_SELF_APPROVAL", "not flagged: {outcome:?}");
}

/// The approval of the waiting list count `pending` by `user` with a valid note.
async fn approve_waiting(
    db: &DatabaseConnection,
    user: i32,
    pending: &negative_stock::PendingReconciliationCount,
) -> Result<negative_stock::NegativeStockResolution, AppError> {
    negative_stock::approve_pending_count(
        db,
        user,
        negative_stock::ApprovePendingCountInput {
            line_id: pending.line_id,
            expected_row_version: pending.row_version,
            reason: "Recompté avec le magasinier".into(),
        },
    )
    .await
}

async fn waiting_counts(db: &DatabaseConnection) -> Vec<negative_stock::PendingReconciliationCount> {
    negative_stock::list_reviews(db, false)
        .await
        .expect("list")
        .pending_counts
}

/// D11 review: the approval of a waiting list count is refused without inv.manage, against another
/// version of its line, or once it was cancelled; the generic count screen cannot approve or post
/// it (it is approved from the list, where the posting closes the item).
#[tokio::test]
async fn a_waiting_list_count_is_approved_only_from_the_list_by_a_manager_against_its_version() {
    use crate::inventory::controls;
    use crate::inventory::domain::{
        ApproveInventoryCountLineInput, PostInventoryCountSessionInput, TransitionInventoryCountSessionInput,
    };
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyesrulescounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let approver = user_with(db, "foureyesrulesapprover", &[crate::rbac::permissions::INV_MANAGE]).await;
    let viewer = user_with(db, "foureyesrulesviewer", &[crate::rbac::permissions::INV_VIEW]).await;
    negative_stock::resolve_by_count(db, counter, resolution(art, loc, 2.0, "Comptage du bac"))
        .await
        .expect("counted");
    let pending = waiting_counts(db).await[0].clone();

    let denied = approve_waiting(db, viewer, &pending).await.expect_err("inv.view only");
    assert!(matches!(denied, AppError::PermissionDenied(_)), "{denied:?}");
    let stale_version = negative_stock::PendingReconciliationCount {
        row_version: pending.row_version + 1,
        ..pending.clone()
    };
    assert_eq!(
        gate(
            approve_waiting(db, approver, &stale_version)
                .await
                .expect_err("another version")
        ),
        "GATE_RECORD_CHANGED_ELSEWHERE"
    );

    // The generic count screen: the line approval and the posting are refused.
    let refused = controls::approve_count_line(
        db,
        ApproveInventoryCountLineInput {
            line_id: pending.line_id,
            expected_row_version: pending.row_version,
            reviewer_id: i64::from(approver),
            reviewer_evidence: "Recompté avec le magasinier".into(),
        },
    )
    .await
    .expect_err("approved from the list only");
    assert_eq!(gate(refused), controls::REFUSED_RECONCILE_LIST_COUNT);
    let session_id = scalar_i64(
        db,
        "SELECT session_id FROM inventory_count_lines WHERE id = ?",
        vec![pending.line_id.into()],
    )
    .await;
    // Even a session put in "approved" by hand is not posted from the count screen.
    exec(
        db,
        "UPDATE inventory_count_sessions SET status = 'approved' WHERE id = ?",
        vec![session_id.into()],
    )
    .await;
    let version = scalar_i64(
        db,
        "SELECT row_version FROM inventory_count_sessions WHERE id = ?",
        vec![session_id.into()],
    )
    .await;
    let refused = controls::post_count_session(
        db,
        PostInventoryCountSessionInput {
            session_id,
            expected_row_version: version,
            actor_id: Some(i64::from(approver)),
        },
    )
    .await
    .expect_err("posted from the list only");
    assert_eq!(gate(refused), controls::REFUSED_RECONCILE_LIST_COUNT);
    assert_eq!(on_hand_milli(db, art, loc).await, -5000, "nothing moved");
    exec(
        db,
        "UPDATE inventory_count_sessions SET status = 'submitted' WHERE id = ?",
        vec![session_id.into()],
    )
    .await;

    // Cancelled from the count screen: no longer waiting, and the item stays open.
    let version = scalar_i64(
        db,
        "SELECT row_version FROM inventory_count_sessions WHERE id = ?",
        vec![session_id.into()],
    )
    .await;
    controls::transition_count_session(
        db,
        TransitionInventoryCountSessionInput {
            session_id,
            expected_row_version: version,
            next_status: "cancelled".into(),
            reason: None,
            actor_id: Some(i64::from(counter)),
        },
    )
    .await
    .expect("cancelled");
    assert_eq!(
        refusal_code(approve_waiting(db, approver, &pending).await.expect_err("cancelled")),
        negative_stock::REFUSED_COUNT_NOT_PENDING
    );
    let list = negative_stock::list_reviews(db, false).await.expect("list");
    assert!(list.pending_counts.is_empty());
    assert_eq!(list.items.len(), 1, "cancelling a waiting count never closes the item");
    assert_eq!(list.items[0].status, "open");
}

/// D11 review (S2, S3, item 11): a waiting count replaced by a new count is cancelled with an audit
/// on its session naming the count that replaced it, and the new count's audit names it; the audit
/// of a count that waits says so; the item stays open until a count is approved and posted, whose
/// approval audit carries the tolerance.
#[tokio::test]
async fn a_replaced_waiting_count_is_audited_and_the_item_stays_open_until_a_valid_count() {
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyesreplcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let approver = user_with(db, "foureyesreplapprover", &[crate::rbac::permissions::INV_MANAGE]).await;
    negative_stock::resolve_by_count(db, counter, resolution(art, loc, 1.0, "Comptage du bac"))
        .await
        .expect("counted");
    let first = waiting_counts(db).await[0].clone();
    let waiting_summary = crate::sync::test_support::scalar_opt_text(
        db,
        "SELECT json_extract(details_json, '$.summary') FROM audit_events
         WHERE action_code = 'inventory.negative_stock.counted' ORDER BY id DESC LIMIT 1",
        vec![],
    )
    .await
    .expect("audit");
    assert!(
        waiting_summary.contains("waiting") && !waiting_summary.contains("resolved"),
        "{waiting_summary}"
    );

    let second = negative_stock::resolve_by_count(db, counter, resolution(art, loc, 1.5, "Second comptage du bac"))
        .await
        .expect("counted again");
    assert!(second.pending_approval);
    let replaced_by = crate::sync::test_support::scalar_opt_text(
        db,
        "SELECT json_extract(details_json, '$.replaced_by_session_code') FROM audit_events
         WHERE action_code = 'inventory.count.replaced' AND actor_id = ?
           AND json_extract(details_json, '$.count_session_code') = ?",
        vec![i64::from(counter).into(), first.session_code.clone().into()],
    )
    .await
    .expect("the replaced count is audited on its session");
    assert_eq!(replaced_by, second.session_code);
    let named = crate::sync::test_support::scalar_opt_text(
        db,
        "SELECT json_extract(details_json, '$.replaced_session_codes') FROM audit_events
         WHERE action_code = 'inventory.negative_stock.counted' ORDER BY id DESC LIMIT 1",
        vec![],
    )
    .await
    .expect("audit");
    assert!(named.contains(&first.session_code), "{named}");
    let list = negative_stock::list_reviews(db, false).await.expect("list");
    assert_eq!(list.items.len(), 1, "replacing a waiting count never closes the item");
    assert_eq!(on_hand_milli(db, art, loc).await, -5000);

    let pending = waiting_counts(db).await[0].clone();
    let done = approve_waiting(db, approver, &pending).await.expect("approved");
    assert_eq!(done.review.expect("item").status, "closed", "a valid count closes it");
    let tolerance = scalar_i64(
        db,
        "SELECT COUNT(*) FROM audit_events
         WHERE action_code = 'inventory.negative_stock.count_approved'
           AND json_extract(details_json, '$.four_eyes_tolerance.above_tolerance_at_entry') = 1
           AND json_extract(details_json, '$.four_eyes_tolerance.at_approval.rule') IS NOT NULL",
        vec![],
    )
    .await;
    assert_eq!(tolerance, 1, "the approval's audit carries the tolerance");
}

/// D11 (CMMS Advisor Q7): the stock moved since a waiting count by a movement RECEIVED from another
/// PC: the approval is refused as stale (the stock on record is compared, whatever moved it) and
/// the counter is told in-app to count again.
#[tokio::test]
async fn a_movement_received_from_another_pc_makes_a_waiting_count_stale_and_the_counter_is_told() {
    let Shortage {
        dbs, mut server, pairs, ..
    } = shortage(2).await;
    let (a, b) = (&dbs[0], &dbs[1]);
    let (art, loc) = pairs[0];
    let counter = user_with(a, "foureyesrecvcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let approver = user_with(a, "foureyesrecvapprover", &[crate::rbac::permissions::INV_MANAGE]).await;
    negative_stock::resolve_by_count(a, counter, resolution(art, loc, 1.0, "Comptage du bac"))
        .await
        .expect("counted");
    let pending = waiting_counts(a).await[0].clone();

    adjust_noted(b, pairs[1].0, pairs[1].1, 2.0, "Retour trouvé sur le poste B")
        .await
        .expect("entry on B");
    sync_applied(&mut server, "B", b).await;
    sync_applied(&mut server, "A", a).await;
    assert_eq!(on_hand_milli(a, art, loc).await, -3000, "B's movement applied on A");

    let stale = approve_waiting(a, approver, &pending).await.expect_err("stale");
    assert_eq!(refusal_code(stale), negative_stock::REFUSED_COUNT_STALE);
    assert_eq!(on_hand_milli(a, art, loc).await, -3000, "nothing posted");
    let notice = |user: i32| {
        scalar_i64(
            a,
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id
             WHERE e.category_code = ? AND n.recipient_user_id = ? AND e.message_key = ?",
            vec![
                negative_stock::RECOUNT_CATEGORY.into(),
                i64::from(user).into(),
                negative_stock::RECOUNT_CATEGORY.into(),
            ],
        )
    };
    assert_eq!(notice(counter).await, 1, "the counter is told to count again");
    assert_eq!(notice(approver).await, 0);
    let params = crate::sync::test_support::scalar_opt_text(
        a,
        "SELECT message_params_json FROM notification_events WHERE category_code = ?",
        vec![negative_stock::RECOUNT_CATEGORY.into()],
    )
    .await
    .expect("params");
    assert!(
        params.contains(&pending.session_code) && params.contains(&pending.location_code),
        "{params}"
    );
    // The notice is named by the line's sync id (the same on every PC), not its local id.
    let line_sync = text_of(
        a,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![pending.line_id.into()],
    )
    .await;
    assert_eq!(
        text_of(
            a,
            "SELECT dedupe_key FROM notification_events WHERE category_code = ?",
            vec![negative_stock::RECOUNT_CATEGORY.into()],
        )
        .await,
        format!("stock-count-recount|{line_sync}")
    );
    let list = negative_stock::list_reviews(a, false).await.expect("list");
    assert_eq!(list.items.len(), 1, "the item stays open");
}

/// D11 review: "above the tolerance" of a received line is judged here from this PC's tolerance and
/// the line's quantities: a line received marked within it, approved by its counter, is still a
/// four-eyes conflict where it is above here (the tests' tolerance is blank: every variance is).
#[tokio::test]
async fn a_received_line_marked_within_the_tolerance_is_judged_on_its_quantities_here() {
    use crate::sync::materialize::ApplyOutcome;
    use sea_orm::TransactionTrait;
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyesrecomputecounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, line) = enter_count(db, counter, art, loc, 2.0).await;
    let sync_of = |sql: &'static str, id: i64| crate::sync::test_support::scalar_opt_text(db, sql, vec![id.into()]);
    let line_sync = sync_of("SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?", line.id)
        .await
        .expect("line");
    let counter_sync = sync_of(
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        i64::from(counter),
    )
    .await
    .expect("counter");
    // The same line as another PC would send it, new here, marked within the tolerance.
    let mut payload = crate::permit::sync_check::local_wire(db, "inventory_count_lines", &line_sync)
        .await
        .expect("wire")
        .expect("line");
    payload.insert("is_critical".into(), serde_json::json!(0));
    payload.insert("approved_by_sync_id".into(), serde_json::json!(counter_sync));
    payload.insert("approved_at".into(), serde_json::json!("2026-10-23T10:00:00Z"));
    payload.insert("approval_note".into(), serde_json::json!("Recompté avec le magasinier"));
    let fresh = crate::sync::identity::new_entity_sync_id();
    let layout = crate::sync::row_layout::layout(db, "inventory_count_lines")
        .await
        .expect("layout");
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        &fresh,
        "upsert",
        &serde_json::to_string(&payload).expect("json"),
    )
    .await
    .expect("apply");
    tx.rollback().await.expect("rollback");
    assert!(
        matches!(&outcome, ApplyOutcome::Conflict { code, .. } if code == "COUNT_SELF_APPROVAL"),
        "{outcome:?}"
    );
}

/// D11 review (B2): a received count line cannot clear the counter held here, nor change it or
/// lower its "above the tolerance" flag without a new entry; with a new entry it is judged as
/// received.
#[tokio::test]
async fn a_received_count_line_keeps_the_counter_and_flag_held_here() {
    use crate::sync::materialize::ApplyOutcome;
    use sea_orm::TransactionTrait;
    let Shortage { dbs, pairs, .. } = shortage(2).await;
    let db = &dbs[0];
    let (art, loc) = pairs[0];
    let counter = user_with(db, "foureyesheldcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let other = user_with(db, "foureyesheldother", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, line) = enter_count(db, counter, art, loc, 2.0).await;
    assert_eq!(line.is_critical, 1);
    let sync_of = |sql: &'static str, id: i64| crate::sync::test_support::scalar_opt_text(db, sql, vec![id.into()]);
    let line_sync = sync_of("SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?", line.id)
        .await
        .expect("line");
    let counter_sync = sync_of(
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        i64::from(counter),
    )
    .await
    .expect("counter");
    let other_sync = sync_of(
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        i64::from(other),
    )
    .await
    .expect("other");
    let held = crate::permit::sync_check::local_wire(db, "inventory_count_lines", &line_sync)
        .await
        .expect("wire")
        .expect("line");
    let received = |changes: &[(&str, serde_json::Value)]| {
        let mut p = held.clone();
        p.insert("approved_by_sync_id".into(), serde_json::json!(counter_sync));
        p.insert("approved_at".into(), serde_json::json!("2026-10-23T10:00:00Z"));
        p.insert("approval_note".into(), serde_json::json!("Recompté avec le magasinier"));
        p.insert("self_approved".into(), serde_json::json!(0));
        p.insert("row_version".into(), serde_json::json!(line.row_version + 1));
        for (k, v) in changes {
            p.insert((*k).into(), v.clone());
        }
        p
    };
    let apply = |payload: serde_json::Map<String, serde_json::Value>| {
        let line_sync = line_sync.clone();
        async move {
            let layout = crate::sync::row_layout::layout(db, "inventory_count_lines")
                .await
                .expect("layout");
            let tx = db.begin().await.expect("begin");
            let outcome = crate::sync::row_codec::apply_row(
                &tx,
                &layout,
                &line_sync,
                "upsert",
                &serde_json::to_string(&payload).expect("json"),
            )
            .await
            .expect("apply");
            tx.rollback().await.expect("rollback");
            outcome
        }
    };
    let code = |outcome: &ApplyOutcome| match outcome {
        ApplyOutcome::Conflict { code, .. } => code.clone(),
        other => format!("{other:?}"),
    };
    let changed = crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_CHANGED;
    // Approved by the real counter: the counter cleared, then changed to someone else.
    let outcome = apply(received(&[("counted_by_sync_id", serde_json::Value::Null)])).await;
    assert_eq!(code(&outcome), changed, "counter cleared: {outcome:?}");
    let outcome = apply(received(&[("counted_by_sync_id", serde_json::json!(other_sync))])).await;
    assert_eq!(code(&outcome), changed, "counter changed: {outcome:?}");
    // The flag lowered without a new entry.
    let outcome = apply(received(&[("is_critical", serde_json::json!(0))])).await;
    assert_eq!(code(&outcome), changed, "flag lowered: {outcome:?}");
    // A new entry by the other person (another quantity), approved by the first counter: a second
    // person, applied.
    let outcome = apply(received(&[
        ("counted_by_sync_id", serde_json::json!(other_sync)),
        ("counted_qty", serde_json::json!(3.0)),
        ("variance_qty", serde_json::json!(8.0)),
    ]))
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "new entry: {outcome:?}");
}

// ── A self-approved count received where four-eyes applies (CMMS Advisor Q8) ──────────────────

/// Each PC's own planning site, made there only (never sent): A's opted out of four-eyes when
/// `a_opts_out`, B's per `b_opts_out` — each PC judges by its current rule.
async fn sites_made_locally(a: &DatabaseConnection, b: &DatabaseConnection, a_opts_out: bool, b_opts_out: bool) {
    for (db, opt_out, device) in [(a, a_opts_out, 7), (b, b_opts_out, 8)] {
        crate::sync::capture::begin_suppression(db).await.expect("suppress");
        crate::inventory::tests::single_site_opting_out(db, &mut RowFactory::for_device(device), opt_out).await;
        crate::sync::capture::end_suppression(db).await.expect("unsuppress");
    }
}

/// The count line of a session on `db`.
async fn line_of(db: &DatabaseConnection, session_code: &str) -> crate::inventory::domain::InventoryCountLine {
    let session = session_by_code(db, session_code).await;
    crate::inventory::controls::list_count_lines(db, session.id)
        .await
        .expect("lines")
        .into_iter()
        .next()
        .expect("its line")
}

async fn notice_events(db: &DatabaseConnection, category: &str) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM notification_events WHERE category_code = ?",
        vec![category.into()],
    )
    .await
}

/// A count self-approved on A under its site's opt-out, received by B where four-eyes applies.
struct HeldCount {
    a: DatabaseConnection,
    b: DatabaseConnection,
    server: TestServer,
    pair_a: (i64, i64),
    pair_b: (i64, i64),
    counter: i32,
    session_code: String,
}

async fn held_count(name: &str) -> HeldCount {
    let Shortage {
        mut dbs,
        mut server,
        pairs,
    } = shortage(2).await;
    let b = dbs.pop().expect("B");
    let a = dbs.pop().expect("A");
    sites_made_locally(&a, &b, true, false).await;
    let counter = user_with(&a, name, &[crate::rbac::permissions::INV_MANAGE]).await;
    let done =
        negative_stock::resolve_by_count(&a, counter, resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage du bac"))
            .await
            .expect("posted under the opt-out");
    assert!(!done.pending_approval && done.self_approved);
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    HeldCount {
        a,
        b,
        server,
        pair_a: pairs[0],
        pair_b: pairs[1],
        counter,
        session_code: done.session_code,
    }
}

/// CMMS Advisor Q8: on a PC where four-eyes applies, a received count approved by its counter above
/// the tolerance is a conflict and its movement is held with it (the stock does not move); the
/// count-approval holders are told once; "keep mine" / "take theirs" are refused; a second approval
/// given where the count is applied releases it: the movement is applied, the conflict closes.
#[tokio::test]
async fn a_self_approved_count_is_held_with_its_movement_until_its_second_approval_arrives() {
    let HeldCount {
        a,
        b,
        mut server,
        pair_a,
        pair_b,
        counter,
        session_code,
    } = held_count("heldsecondcounter").await;
    assert_eq!(on_hand_milli(&a, pair_a.0, pair_a.1).await, 0, "posted on A");
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        -5000,
        "held on B: the stock does not move"
    );
    assert_eq!(
        conflict_kinds(&b).await,
        vec!["inventory_count_lines:COUNT_SELF_APPROVAL".to_string()]
    );
    assert!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM sync_inbox WHERE entity_type = 'inventory_transactions' AND apply_status = 'deferred'",
            vec![],
        )
        .await
            >= 1,
        "the movement waits with its line"
    );
    let held_notice = crate::inventory::count_four_eyes::SECOND_APPROVAL_CATEGORY;
    assert_eq!(
        notice_events(&b, held_notice).await,
        1,
        "the count-approval holders are told"
    );
    // The sync centre names the count held: its reference, its counter, when it was posted (Q12).
    let listed = crate::sync::queries::list_sync_conflicts(&b, Default::default())
        .await
        .expect("conflicts");
    let hold = listed
        .iter()
        .find_map(|c| c.count_hold.clone())
        .expect("the held count is named");
    assert_eq!(hold.session_code.as_deref(), Some(session_code.as_str()));
    assert_eq!(hold.counter.as_deref(), Some("heldsecondcounter"));
    assert!(hold.posted_at.is_some(), "{hold:?}");

    // "Keep mine" / "take theirs" are refused: settled where the count is applied.
    use sea_orm::ConnectionTrait;
    let conflict = b
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT id, row_version FROM sync_conflicts WHERE conflict_type = 'COUNT_SELF_APPROVAL'".to_string(),
        ))
        .await
        .expect("query")
        .expect("conflict");
    let (conflict_id, version): (i64, i64) = (
        conflict.try_get("", "id").expect("id"),
        conflict.try_get("", "row_version").expect("version"),
    );
    let admin_b = i64::from(second_person(&b, counter).await);
    for action in ["accept_local", "accept_remote", "dismiss"] {
        let refused = crate::sync::queries::resolve_sync_conflict(
            &b,
            admin_b,
            crate::sync::domain::ResolveSyncConflictInput {
                conflict_id,
                expected_row_version: version,
                action: action.into(),
                resolution_note: Some("Choix de la version de ce poste".into()),
            },
        )
        .await
        .expect_err(action);
        assert!(
            format!("{refused:?}").contains("GATE_SYNC_CONFLICT_SETTLED_AT_ORIGIN"),
            "{action}: {refused:?}"
        );
    }

    // Judged again (its inbox row tried anew, twice): still held, told once — the notice is named
    // by the count, not by the attempt.
    for _ in 0..2 {
        exec(
            &b,
            "UPDATE sync_inbox SET apply_status = 'pending' WHERE entity_type = 'inventory_count_lines'
               AND apply_status = 'conflict'",
            vec![],
        )
        .await;
        let judged = crate::sync::materialize::materialize_inbox_batch(&b, Some(2000))
            .await
            .expect("judged again");
        assert!(judged.conflicts >= 1, "refused again");
    }
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, -5000);
    assert_eq!(notice_events(&b, held_notice).await, 1, "never repeated");

    // The second approval, where the count is applied: never by its counter.
    let line = line_of(&a, &session_code).await;
    let second = |by: i32, line: &crate::inventory::domain::InventoryCountLine| {
        crate::inventory::controls::second_approve_count_line(
            &a,
            crate::inventory::domain::SecondApproveInventoryCountLineInput {
                line_id: line.id,
                expected_row_version: line.row_version,
                reviewer_id: i64::from(by),
                reviewer_evidence: "Comptage vérifié par un second".into(),
            },
        )
    };
    assert_eq!(
        gate(second(counter, &line).await.expect_err("the counter")),
        crate::inventory::controls::REFUSED_SECOND_APPROVAL_SELF
    );
    let approver = second_person(&a, counter).await;
    let approved = second(approver, &line).await.expect("second approval");
    assert_eq!(approved.second_approved_by_id, Some(i64::from(approver)));
    assert_eq!(
        gate(second(approver, &approved).await.expect_err("twice")),
        crate::inventory::controls::REFUSED_SECOND_APPROVAL_NOT_NEEDED
    );

    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        0,
        "released on B: the movement is applied with its line"
    );
    assert!(conflict_kinds(&b).await.is_empty(), "the conflict closed by itself");
    assert_eq!(notice_events(&b, held_notice).await, 1);

    // The second approval held on B is kept: a received row cannot remove or change it.
    let line_sync = text_of(
        &b,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE second_approved_by_id IS NOT NULL",
        vec![],
    )
    .await;
    let held = crate::permit::sync_check::local_wire(&b, "inventory_count_lines", &line_sync)
        .await
        .expect("wire")
        .expect("line");
    let counter_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![counter.into()],
    )
    .await;
    let changed = crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_CHANGED;
    for (what, key, value) in [
        ("removed", "second_approved_by_sync_id", serde_json::Value::Null),
        (
            "given to another",
            "second_approved_by_sync_id",
            serde_json::json!(counter_sync),
        ),
        (
            "redated",
            "second_approved_at",
            serde_json::json!("2026-10-24T23:59:00Z"),
        ),
    ] {
        let mut received = held.clone();
        received.insert(key.into(), value);
        let verdict = crate::inventory::count_four_eyes::inbound_count_line_check(&b, &line_sync, &received)
            .await
            .expect("judged");
        assert_eq!(verdict_code(verdict.as_ref()), changed, "second approval {what}");
    }
}

/// `pass`, `wait` or the conflict code of a received row's verdict.
fn verdict_code(verdict: Option<&crate::settings::site::InboundVerdict>) -> String {
    match verdict {
        None => "pass".into(),
        Some(crate::settings::site::InboundVerdict::Defer(_)) => "wait".into(),
        Some(crate::settings::site::InboundVerdict::Conflict { code, .. }) => (*code).to_string(),
    }
}

/// Review of a9c38354 (B1): the exchange can end between a count's movement and the line version
/// naming it. With an earlier version of the line already applied here (entered, not yet
/// approved), the movement still waits for the version naming it, judged with it: the stock does
/// not move while that version is refused.
#[tokio::test]
async fn a_count_movement_waits_for_the_line_version_naming_it_across_exchanges() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sites_made_locally(&a, &b, true, false).await;
    let counter = user_with(&a, "pagesplitcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let pair_a = (article.rowid, location.rowid);
    let pair_b = local_pair(&b, &article, &location).await;
    let entered = enter_count(&a, counter, pair_a.0, pair_a.1, 7.0).await;
    let line_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![entered.1.id.into()],
    )
    .await;
    let session_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![entered.0.id.into()],
    )
    .await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM inventory_count_lines WHERE entity_sync_id = ? AND approved_by_id IS NULL",
            vec![line_sync.clone().into()],
        )
        .await,
        1,
        "the line as entered is applied on B"
    );

    // Approved by its counter under A's opt-out, then posted on A.
    post_count_approved_by(&a, counter, counter, entered).await;
    assert_eq!(on_hand_milli(&a, pair_a.0, pair_a.1).await, 7000);
    sync_applied(&mut server, "A", &a).await;

    // B's exchange ends between the movement and the line version naming it.
    server.hold_back("B", &line_sync);
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_ADJUST").await,
        (0, 0),
        "the movement waits for the line version naming it"
    );
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);

    // That version arrives in the next exchange, refused here: the movement stays held with it.
    server.release("B");
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        conflict_kinds(&b).await,
        vec!["inventory_count_lines:COUNT_SELF_APPROVAL".to_string()]
    );
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (0, 0));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        10_000,
        "held: the stock does not move"
    );
}

/// Review of a9c38354 (S2): a held adjustment is released only together with its reversal, in one
/// transaction. When the reversal cannot be applied (it waits for a row of its own, here a person
/// not applied on B yet), the adjustment is not applied alone either.
#[tokio::test]
async fn a_held_adjustment_is_never_applied_without_its_reversal() {
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let HeldCount {
        a,
        b,
        mut server,
        pair_a,
        pair_b,
        counter,
        session_code,
    } = held_count("heldgroupcounter").await;
    exec_unsent(
        &a,
        "UPDATE stock_balances SET on_hand_qty = on_hand_qty + 10 WHERE article_id = ? AND location_id = ?",
        vec![pair_a.0.into(), pair_a.1.into()],
    )
    .await;
    let session = session_by_code(&a, &session_code).await;
    crate::inventory::controls::reverse_count_session(
        &a,
        ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Pas de seconde approbation possible".into(),
            actor_id: Some(i64::from(second_person(&a, counter).await)),
        },
    )
    .await
    .expect("reversed");
    sync_applied(&mut server, "A", &a).await;
    let session_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE session_code = ?",
        vec![session_code.clone().into()],
    )
    .await;
    let reversal = movement_sync_id(&a, &session_sync, "COUNT_REVERSAL").await;
    // The reversal reaches B naming a person not applied there: it waits, for a reason of its own.
    {
        let row = server
            .state
            .get_mut(&("inventory_transactions".to_string(), reversal.clone()))
            .expect("the reversal is on the server");
        let mut payload: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&row.payload_json).expect("payload");
        payload.insert(
            "performed_by_sync_id".into(),
            serde_json::json!("a-person-not-applied-on-b"),
        );
        row.payload_json = serde_json::to_string(&payload).expect("json");
        row.payload_hash = crate::sync::queries::hash_payload(&row.payload_json);
    }
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_REVERSAL").await,
        (0, 0),
        "the reversal waits"
    );
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_ADJUST").await,
        (0, 0),
        "the adjustment is not applied without its reversal"
    );
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        -5000,
        "the stock does not move"
    );

    // Review of 435c9410 (SF3): the missing person arrives — the group is tried again and recovers:
    // the adjustment and its reversal are applied together, the stock nets to zero, the conflict
    // closes.
    let late = crate::auth::scope_tests::user(&b, "heldgrouplate").await;
    exec_unsent(
        &b,
        "UPDATE user_accounts SET entity_sync_id = 'a-person-not-applied-on-b' WHERE id = ?",
        vec![late.into()],
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried again");
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (1, 1));
    assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (1, 1));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        -5000,
        "both together: net zero"
    );
    assert!(conflict_kinds(&b).await.is_empty(), "the conflict closed by itself");
}

/// Review of a9c38354 (S1, S3): a received count line is checked on what it names — a second
/// approval by its counter, undated, by someone without the count-approval right here, or by
/// someone not applied here yet (it waits); a "reversal" that is not the reversal of this count
/// (another movement, or one not arrived: it waits).
#[tokio::test]
async fn a_received_count_line_is_checked_on_its_second_approval_and_its_reversal() {
    let HeldCount {
        a,
        b,
        counter,
        session_code,
        ..
    } = held_count("forgedsecondcounter").await;
    let line_sync = text_of(
        &a,
        "SELECT cl.entity_sync_id FROM inventory_count_lines cl
         JOIN inventory_count_sessions s ON s.id = cl.session_id WHERE s.session_code = ?",
        vec![session_code.clone().into()],
    )
    .await;
    let user_sync = |db: &DatabaseConnection, id: i32| {
        let db = db.clone();
        async move {
            text_of(
                &db,
                "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
                vec![id.into()],
            )
            .await
        }
    };
    let counter_sync = user_sync(&a, counter).await;
    let admin_sync = text_of(
        &b,
        "SELECT entity_sync_id FROM user_accounts WHERE username = 'admin'",
        vec![],
    )
    .await;
    let viewer = user_with(&b, "forgedsecondviewer", &[crate::rbac::permissions::INV_VIEW]).await;
    let viewer_sync = user_sync(&b, viewer).await;
    // The line as A sends it: posted, approved by its counter, held on B.
    let posted = wire_row(&a, "inventory_count_lines", &line_sync).await;
    let judge = |changes: Vec<(&'static str, serde_json::Value)>| {
        let mut received = posted.clone();
        for (k, v) in changes {
            received.insert(k.into(), v);
        }
        let (b, line_sync) = (&b, line_sync.clone());
        async move {
            let verdict = crate::inventory::count_four_eyes::inbound_count_line_check(b, &line_sync, &received)
                .await
                .expect("judged");
            verdict_code(verdict.as_ref())
        }
    };
    let second = |by: serde_json::Value, at: serde_json::Value| {
        vec![
            ("second_approved_by_sync_id", by),
            ("second_approved_at", at),
            (
                "second_approval_note",
                serde_json::json!("Comptage vérifié par un second"),
            ),
        ]
    };
    let at = serde_json::json!("2026-10-24T10:00:00Z");
    let invalid = crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_INVALID;
    let held = crate::inventory::count_four_eyes::SYNC_SELF_APPROVAL;
    assert_eq!(
        judge(second(serde_json::json!(counter_sync), at.clone())).await,
        invalid,
        "by its counter"
    );
    assert_eq!(
        judge(second(serde_json::json!(admin_sync), serde_json::Value::Null)).await,
        invalid,
        "undated"
    );
    assert_eq!(
        judge(second(serde_json::json!(viewer_sync), at.clone())).await,
        held,
        "without the count-approval right here"
    );
    assert_eq!(
        judge(second(serde_json::json!("a-person-not-applied-on-b"), at.clone())).await,
        "wait",
        "its second approver not applied here yet"
    );
    assert_eq!(
        judge(second(serde_json::json!(admin_sync), at.clone())).await,
        "pass",
        "a valid second approval"
    );
    // Q15: a second approval only of a variance approved by its counter.
    let mut by_another = second(serde_json::json!(admin_sync), at.clone());
    by_another.push(("approved_by_sync_id", serde_json::json!(admin_sync)));
    assert_eq!(judge(by_another).await, invalid, "approved by another person");

    // Its "reversal": the count's own adjustment (waiting on B), another movement held on B, one
    // not arrived.
    let session_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE session_code = ?",
        vec![session_code.clone().into()],
    )
    .await;
    let adjustment = movement_sync_id(&a, &session_sync, "COUNT_ADJUST").await;
    let other_movement = text_of(
        &b,
        "SELECT entity_sync_id FROM inventory_transactions ORDER BY id LIMIT 1",
        vec![],
    )
    .await;
    let reversed = |id: &str| vec![("reversed_transaction_sync_id", serde_json::json!(id))];
    assert_eq!(judge(reversed(&adjustment)).await, invalid, "its own adjustment");
    assert_eq!(judge(reversed(&other_movement)).await, invalid, "another movement");
    assert_eq!(
        judge(reversed("a-movement-not-arrived")).await,
        "wait",
        "not arrived yet"
    );

    // Review of 2e7a433c (§3): a second approval of a line without variance contradicts itself
    // (nothing posted: a count without variance moves nothing).
    let system = posted
        .get("system_qty")
        .and_then(serde_json::Value::as_f64)
        .expect("system quantity");
    let mut no_variance = second(serde_json::json!(admin_sync), at.clone());
    no_variance.extend([
        ("counted_qty", serde_json::json!(system)),
        ("variance_qty", serde_json::json!(0.0)),
        ("posted_transaction_sync_id", serde_json::Value::Null),
    ]);
    match crate::inventory::count_four_eyes::inbound_count_line_check(&b, &line_sync, &{
        let mut received = posted.clone();
        for (k, v) in no_variance {
            received.insert(k.into(), v);
        }
        received
    })
    .await
    .expect("judged")
    {
        Some(crate::settings::site::InboundVerdict::Conflict { code, message }) => {
            assert_eq!(code, invalid, "a second approval of no variance");
            assert!(
                message.contains("carries a second approval although it is not a variance approved by its counter"),
                "{message}"
            );
        }
        other => panic!("a second approval of no variance is refused: {other:?}"),
    }
    // Within the tolerance here the optional second approval is only checked to be well formed: a
    // person without the count-approval right here may give it (deliberate relaxation, Q15).
    tolerance_here_only(&b, crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD, "50").await;
    let mut within = second(serde_json::json!(viewer_sync), at.clone());
    within.extend([
        ("system_qty", serde_json::json!(10.0)),
        ("counted_qty", serde_json::json!(9.0)),
        ("variance_qty", serde_json::json!(-1.0)),
        ("is_critical", serde_json::json!(false)),
        ("posted_transaction_sync_id", serde_json::Value::Null),
    ]);
    assert_eq!(
        judge(within).await,
        "pass",
        "within here, seconded by a person without inv.manage"
    );
}

/// CMMS Advisor Q8: the count's reversal issued where it is applied: on the PC holding it, the held
/// adjustment and its reversal are applied together (net zero), the conflict closes, the counter is
/// told to count again and the stock to reconcile stays open.
#[tokio::test]
async fn a_held_count_reversed_where_it_is_applied_nets_to_zero_and_closes() {
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let HeldCount {
        a,
        b,
        mut server,
        pair_a,
        pair_b,
        counter,
        session_code,
    } = held_count("heldreversecounter").await;
    // A's reversal must not take A below zero (it brings back the −5): stock found on A since,
    // repaired on A only, so B's stock is the count's alone.
    exec_unsent(
        &a,
        "UPDATE stock_balances SET on_hand_qty = on_hand_qty + 10 WHERE article_id = ? AND location_id = ?",
        vec![pair_a.0.into(), pair_a.1.into()],
    )
    .await;
    let session = session_by_code(&a, &session_code).await;
    crate::inventory::controls::reverse_count_session(
        &a,
        ReverseInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            reason: "Pas de seconde approbation possible".into(),
            actor_id: Some(i64::from(second_person(&a, counter).await)),
        },
    )
    .await
    .expect("reversed");
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    let session_sync_id = text_of(
        &b,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE session_code = ?",
        vec![session_code.clone().into()],
    )
    .await;
    assert_eq!(count_movements(&b, &session_sync_id, "COUNT_ADJUST").await, (1, 1));
    assert_eq!(count_movements(&b, &session_sync_id, "COUNT_REVERSAL").await, (1, 1));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        -5000,
        "the adjustment and its reversal together: net zero"
    );
    assert!(conflict_kinds(&b).await.is_empty(), "the conflict closed by itself");
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id
             WHERE e.category_code = ? AND e.message_key = ? AND n.recipient_user_id = ?",
            vec![
                negative_stock::RECOUNT_CATEGORY.into(),
                crate::inventory::count_four_eyes::RECOUNT_AFTER_REVERSAL_MESSAGE.into(),
                i64::from(counter).into(),
            ],
        )
        .await,
        1,
        "the counter is told to count again"
    );
    let line_sync = text_of(
        &b,
        "SELECT cl.entity_sync_id FROM inventory_count_lines cl
         JOIN inventory_count_sessions s ON s.id = cl.session_id WHERE s.session_code = ?",
        vec![session_code.clone().into()],
    )
    .await;
    assert_eq!(
        text_of(
            &b,
            "SELECT dedupe_key FROM notification_events WHERE message_key = ?",
            vec![crate::inventory::count_four_eyes::RECOUNT_AFTER_REVERSAL_MESSAGE.into()],
        )
        .await,
        format!("count-recount|{line_sync}"),
        "named by the line's sync id"
    );
    let list = negative_stock::list_reviews(&b, false).await.expect("list");
    assert_eq!(list.items.len(), 1, "the stock to reconcile stays open");
}

/// CMMS Advisor Q8: where the site opted out of four-eyes here too, the received self-approved count
/// is accepted on arrival (flagged), with its movement.
#[tokio::test]
async fn a_self_approved_count_is_accepted_where_the_site_opted_out_here_too() {
    let Shortage {
        mut dbs,
        mut server,
        pairs,
    } = shortage(2).await;
    let b = dbs.pop().expect("B");
    let a = dbs.pop().expect("A");
    sites_made_locally(&a, &b, true, true).await;
    let counter = user_with(&a, "heldoptoutcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let done =
        negative_stock::resolve_by_count(&a, counter, resolution(pairs[0].0, pairs[0].1, 0.0, "Comptage du bac"))
            .await
            .expect("posted");
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        on_hand_milli(&b, pairs[1].0, pairs[1].1).await,
        0,
        "accepted with its movement"
    );
    assert!(conflict_kinds(&b).await.is_empty());
    assert_eq!(line_of(&b, &done.session_code).await.self_approved, 1, "flagged");
}

// ── Received lines judged on their quantities and entry cost; first join (CMMS Advisor Q9, Q10) ──

/// Review of 9ebd482c (blocking) and CMMS Advisor Q10: a received line approved by its counter is
/// judged on its own quantities — its variance must be its counted less its system quantity, or the
/// record is refused — with a value threshold set: at the unit cost it was entered with when that
/// cost is consistent with its stored value and within 10 % of the average cost here, otherwise at
/// the cost here (a forged or inconsistent cost never makes it more lenient).
#[tokio::test]
async fn a_received_line_is_judged_on_its_quantities_and_the_cost_it_was_entered_with() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    // The average cost here; the value threshold set, the percent blank.
    exec(
        &db,
        "UPDATE stock_balances SET moving_avg_unit_cost = 10.5 WHERE article_id = ? AND location_id = ?",
        vec![article.rowid.into(), location.rowid.into()],
    )
    .await;
    crate::settings::set_setting(
        &db,
        crate::inventory::count_four_eyes::SETTING_VALUE_THRESHOLD,
        "tenant",
        "50",
        None,
        "test",
    )
    .await
    .expect("value threshold");
    let counter = user_with(&db, "entrycostcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, line) = enter_count(&db, counter, article.rowid, location.rowid, 5.0).await;
    assert_eq!(line.is_critical, 1, "5 × 10.5 = 52.5 is above 50 here");
    let line_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![line.id.into()],
    )
    .await;
    let counter_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![counter.into()],
    )
    .await;
    let entered = crate::permit::sync_check::local_wire(&db, "inventory_count_lines", &line_sync)
        .await
        .expect("wire")
        .expect("line");
    assert_eq!(
        entered.get("entry_unit_cost"),
        Some(&serde_json::json!(10.5)),
        "kept at entry"
    );
    assert_eq!(entered.get("entry_variance_value"), Some(&serde_json::json!(-52.5)));
    // The same line as another PC would send it, new here: approved by its counter, judged within
    // the tolerance there (not flagged as a self-approval).
    let received = |changes: Vec<(&'static str, serde_json::Value)>| {
        let mut p = entered.clone();
        for (k, v) in [
            ("is_critical", serde_json::json!(0)),
            ("approved_by_sync_id", serde_json::json!(counter_sync)),
            ("approved_at", serde_json::json!("2026-10-24T10:00:00Z")),
            ("approval_note", serde_json::json!("Recompté avec le magasinier")),
            ("self_approved", serde_json::json!(0)),
        ] {
            p.insert(k.into(), v);
        }
        for (k, v) in changes {
            p.insert(k.into(), v);
        }
        let db = &db;
        async move {
            let fresh = crate::sync::identity::new_entity_sync_id();
            let verdict = crate::inventory::count_four_eyes::inbound_count_line_check(db, &fresh, &p)
                .await
                .expect("judged");
            verdict_code(verdict.as_ref())
        }
    };
    let entry = |cost: f64, value: f64| {
        vec![
            ("entry_unit_cost", serde_json::json!(cost)),
            ("entry_variance_value", serde_json::json!(value)),
        ]
    };
    let held = crate::inventory::count_four_eyes::SYNC_SELF_APPROVAL;
    // Entered at a cost of 10 (5 × 10 = 50: within), 10.5 here: a drift within 10 %.
    assert_eq!(received(entry(10.0, -50.0)).await, "pass", "no false conflict");
    // A cost far from the one here (5, value 25): not trusted, judged at 10.5.
    assert_eq!(received(entry(5.0, -25.0)).await, held, "a forged cost");
    // A value that is not the variance × the stored cost: judged at the cost here.
    assert_eq!(received(entry(10.0, -10.0)).await, held, "an inconsistent value");
    // No cost stored (a PC before the update): judged at the cost here.
    assert_eq!(
        received(vec![
            ("entry_unit_cost", serde_json::Value::Null),
            ("entry_variance_value", serde_json::Value::Null),
        ])
        .await,
        held,
        "judged at the cost here"
    );
    // A variance of 0 although the counted quantity is not the system quantity: refused.
    let mut zero = entry(10.0, 0.0);
    zero.push(("variance_qty", serde_json::json!(0.0)));
    assert_eq!(
        received(zero).await,
        crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_INVALID,
        "variance 0, counted 5 of 10"
    );
}

/// A snapshot of every plant row on the server, in its order, stored in `db`'s inbox under `batch`
/// (a `snapshot-…` batch this PC asked for) and materialized.
async fn snapshot_into(server: &TestServer, db: &DatabaseConnection, batch: &str) {
    let mut rows: Vec<_> = server
        .state
        .iter()
        .filter(|((entity_type, _), _)| {
            crate::sync::registry::entity(entity_type).map(|e| e.scope) != Some(crate::sync::registry::SyncScope::User)
        })
        .collect();
    rows.sort_by_key(|(_, row)| row.seq);
    for ((entity_type, sync_id), row) in rows {
        exec(
            db,
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                                     row_version, payload_json, payload_hash, apply_status)
             VALUES (?, 'c1', ?, ?, ?, ?, ?, ?, 'pending')",
            vec![
                batch.into(),
                entity_type.clone().into(),
                sync_id.clone().into(),
                row.operation.clone().into(),
                row.row_version.into(),
                row.payload_json.clone().into(),
                row.payload_hash.clone().into(),
            ],
        )
        .await;
    }
    crate::sync::materialize::materialize_inbox(db, 200, true)
        .await
        .expect("materialize");
}

/// CMMS Advisor Q9: a PC joining the plant receives its history in the snapshot of its first join:
/// a count self-approved on another PC, where four-eyes applies on the PC joining, is applied as
/// history (no four-eyes judgement) with its movement, its self-approval flag kept for review.
#[tokio::test]
async fn a_self_approved_count_received_in_a_first_join_snapshot_is_applied_as_history() {
    let HeldCount {
        server, session_code, ..
    } = held_count("firstjoincounter").await;
    let c = started_device_db().await;
    crate::sync::capture::begin_suppression(&c).await.expect("suppress");
    crate::inventory::tests::single_site_opting_out(&c, &mut RowFactory::for_device(9), false).await;
    crate::sync::capture::end_suppression(&c).await.expect("unsuppress");
    // The snapshot of C's first join: every plant row on the server, in its order.
    snapshot_into(&server, &c, "snapshot-all-firstjoin").await;
    let kinds = conflict_kinds(&c).await;
    assert!(
        !kinds.iter().any(|k| k.starts_with("inventory_")),
        "no count held on a first join: {kinds:?}"
    );
    let line = line_of(&c, &session_code).await;
    assert_eq!(
        line.self_approved, 1,
        "the self-approval flag is kept, shown for review"
    );
    assert!(line.posted_transaction_id.is_some(), "applied with its movement");
    let (art, loc) = (line.article_id, line.location_id);
    assert_eq!(
        on_hand_milli(&c, art, loc).await,
        0,
        "the count's stock, as on the other PCs"
    );
}

// ── Re-bootstrap judged, reversal checked (reviews of 536cb792 and 435c9410) ──────────────────

/// A row stored in `db`'s inbox as received under `batch`, with `status`.
async fn inbox_put(
    db: &DatabaseConnection,
    batch: &str,
    entity_type: &str,
    sync_id: &str,
    row_version: i64,
    payload: &serde_json::Map<String, serde_json::Value>,
    status: &str,
) {
    let json = serde_json::to_string(payload).expect("json");
    exec(
        db,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                                 row_version, payload_json, payload_hash, apply_status)
         VALUES (?, 'c1', ?, ?, 'upsert', ?, ?, ?, ?)",
        vec![
            batch.into(),
            entity_type.into(),
            sync_id.into(),
            row_version.into(),
            json.clone().into(),
            crate::sync::queries::hash_payload(&json).into(),
            status.into(),
        ],
    )
    .await;
}

/// The verdict on a received count line, judged as a row of `batch` (snapshot or exchange).
async fn line_judged_in(
    db: &DatabaseConnection,
    batch: &str,
    sync_id: &str,
    received: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let verdict = crate::sync::materialize::judged_as_row_of_batch(
        batch,
        crate::inventory::count_four_eyes::inbound_count_line_check(db, sync_id, received),
    )
    .await
    .expect("judged");
    verdict_code(verdict.as_ref())
}

/// The sync id of the count line of a session on `db`.
async fn line_sync_of(db: &DatabaseConnection, session_code: &str) -> String {
    text_of(
        db,
        "SELECT cl.entity_sync_id FROM inventory_count_lines cl
         JOIN inventory_count_sessions s ON s.id = cl.session_id WHERE s.session_code = ?",
        vec![session_code.into()],
    )
    .await
}

async fn session_sync_of(db: &DatabaseConnection, session_code: &str) -> String {
    text_of(
        db,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE session_code = ?",
        vec![session_code.into()],
    )
    .await
}

/// Review of 536cb792 (S1): a snapshot asked for again (a re-bootstrap after the server refused the
/// checkpoint, e.g. after a server restore) brings back a count line held here in conflict, with its
/// held adjustment: it is not a first join — the line is judged again, both stay held and the stock
/// does not move. The movement is judged with its line in the line's own context.
#[tokio::test]
async fn a_re_bootstrap_keeps_a_held_count_held() {
    let HeldCount {
        a,
        b,
        server,
        pair_b,
        session_code,
        ..
    } = held_count("rebootstrapcounter").await;
    let session_sync = session_sync_of(&a, &session_code).await;
    let adjustment = movement_sync_id(&a, &session_sync, "COUNT_ADJUST").await;
    let adjustment_row = wire_row(&a, "inventory_transactions", &adjustment).await;
    // The movement judged as a snapshot row: its line's version came from an exchange, judged so.
    let verdict = crate::sync::materialize::judged_as_row_of_batch(
        "snapshot-all-rebootstrap",
        crate::inventory::count_four_eyes::inbound_count_movement_hold(&b, &adjustment, &adjustment_row),
    )
    .await
    .expect("judged");
    assert_eq!(verdict_code(verdict.as_ref()), "wait", "held with its line");

    snapshot_into(&server, &b, "snapshot-all-rebootstrap").await;
    assert!(
        conflict_kinds(&b)
            .await
            .contains(&"inventory_count_lines:COUNT_SELF_APPROVAL".to_string()),
        "still held"
    );
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_ADJUST").await,
        (0, 0),
        "the adjustment is not applied by the snapshot"
    );
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        -5000,
        "the stock does not move"
    );
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM inventory_count_lines WHERE entity_sync_id = ?",
            vec![line_sync_of(&a, &session_code).await.into()],
        )
        .await,
        0,
        "the line is not applied"
    );
}

/// Review of 536cb792 (S1): a snapshot row is the plant's history only for a line new here, and only
/// until this PC completed a full pull; a line already here, or a version from an exchange, is judged.
#[tokio::test]
async fn a_snapshot_row_is_history_only_for_a_line_new_here_on_a_first_join() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let counter = user_with(&db, "snapshotjudgedcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, line) = enter_count(&db, counter, article.rowid, location.rowid, 5.0).await;
    assert_eq!(line.is_critical, 1, "blank tolerance: strict");
    let line_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![line.id.into()],
    )
    .await;
    let counter_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![counter.into()],
    )
    .await;
    // Approved by its counter, not flagged: four-eyes refuses it wherever it is judged.
    let mut received = crate::permit::sync_check::local_wire(&db, "inventory_count_lines", &line_sync)
        .await
        .expect("wire")
        .expect("line");
    for (k, v) in [
        ("approved_by_sync_id", serde_json::json!(counter_sync)),
        ("approved_at", serde_json::json!("2026-10-25T09:00:00Z")),
        ("approval_note", serde_json::json!("Recompté avec le magasinier")),
    ] {
        received.insert(k.into(), v);
    }
    let held = crate::inventory::count_four_eyes::SYNC_SELF_APPROVAL;
    let snapshot = "snapshot-all-join";
    assert_eq!(
        line_judged_in(&db, snapshot, &line_sync, &received).await,
        held,
        "a line already here: judged"
    );
    let fresh = crate::sync::identity::new_entity_sync_id();
    assert_eq!(
        line_judged_in(&db, snapshot, &fresh, &received).await,
        "pass",
        "new here, first join: history"
    );
    assert_eq!(
        line_judged_in(&db, "delta-42", &fresh, &received).await,
        held,
        "from an exchange: judged"
    );
    crate::commands::product_license::record_activation_claim_for_tests(&db, "tenant-plant-a", "fp-a", "token-a")
        .await
        .expect("claim");
    crate::sync::plant_join::record_complete_pull(&db, "tenant-plant-a")
        .await
        .expect("complete pull");
    assert_eq!(
        line_judged_in(&db, snapshot, &fresh, &received).await,
        held,
        "after a complete pull, a snapshot is no first join"
    );
    // Review of 93817231 (NOTE 1): the pull was of one organisation; re-activated into another one
    // (no local data reset), this PC's first snapshot of that one is its first join.
    crate::commands::product_license::record_activation_claim_for_tests(&db, "tenant-plant-b", "fp-a", "token-a")
        .await
        .expect("claim");
    assert_eq!(
        line_judged_in(&db, snapshot, &fresh, &received).await,
        "pass",
        "a complete pull of another organisation: first join"
    );
}

/// CMMS Advisor Q14: a count from the list within the four-eyes tolerance records the person
/// counting as its reviewer; no second person was needed, so it is not listed as an
/// auto-approbation (above the tolerance, approved by its counter, it is:
/// `a_count_posted_twice_is_an_item_that_a_new_count_closes`).
#[tokio::test]
async fn a_list_count_within_the_tolerance_approved_by_its_counter_is_no_auto_approbation() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let user = user_with(&db, "q14withincounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (art, loc) = (article.rowid, location.rowid);
    let session = approve_count(&db, user, enter_count(&db, user, art, loc, 7.0).await).await;
    let session_sync_id = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    post_on(&db, user, &session_sync_id).await;
    // The same posting made again under its own id (an older version's PC): a count posted twice.
    let first = movement_sync_id(&db, &session_sync_id, "COUNT_ADJUST").await;
    exec(
        &db,
        "INSERT INTO inventory_transactions
            (entity_sync_id, article_id, warehouse_id, location_id, movement_type, quantity, source_type,
             source_id, source_ref, reason, performed_by_id, performed_at, row_version)
         SELECT ?, article_id, warehouse_id, location_id, movement_type, quantity, source_type, source_id,
                source_ref, reason, performed_by_id, '2026-10-01T09:00:00Z', 1
         FROM inventory_transactions WHERE entity_sync_id = ?",
        vec![crate::sync::identity::new_entity_sync_id().into(), first.into()],
    )
    .await;
    exec(
        &db,
        "UPDATE stock_balances SET on_hand_qty = on_hand_qty - 3, available_qty = available_qty - 3
         WHERE article_id = ? AND location_id = ?",
        vec![art.into(), loc.into()],
    )
    .await;
    assert_eq!(
        negative_stock::list_reviews(&db, false)
            .await
            .expect("list")
            .double_postings
            .len(),
        1
    );
    crate::settings::set_setting(
        &db,
        crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD,
        "tenant",
        "50",
        None,
        "test",
    )
    .await
    .expect("percent threshold");
    // 3 counted, 4 on record: 25 %, within 50 %: posted at once, its counter as its reviewer.
    let resolved =
        negative_stock::resolve_by_count(&db, user, resolution(art, loc, 3.0, "Recompté après double passage"))
            .await
            .expect("posted");
    assert!(!resolved.pending_approval && !resolved.self_approved);
    let all = negative_stock::list_reviews(&db, true).await.expect("list");
    let closed_by = all.closed_double_postings[0].closed_by.as_ref().expect("closed by");
    assert_eq!(closed_by.session_code, resolved.session_code);
    assert!(closed_by.approved_by.is_some());
    assert_eq!(
        closed_by.approved_by, closed_by.counted_by,
        "its counter is its reviewer"
    );
    assert!(!closed_by.self_approved, "within the tolerance: no auto-approbation");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM inventory_count_lines cl JOIN inventory_count_sessions s ON s.id = cl.session_id
             WHERE s.session_code = ? AND cl.is_critical = 0 AND cl.self_approved = 0
               AND cl.approved_by_id = cl.counted_by_id",
            vec![resolved.session_code.clone().into()],
        )
        .await,
        1
    );
}

/// CMMS Advisor Q14: a received count line's "above the tolerance" flag is this PC's judgement. A
/// line approved by its counter whose sender cleared the flag, above the tolerance here (blank:
/// strict), is stored above it and shown as an auto-approbation — here a line new here in the
/// snapshot of a first join, applied as the plant's history (Q9), not flagged as a self-approval.
#[tokio::test]
async fn a_received_line_cleared_of_its_critique_but_above_here_is_an_auto_approbation() {
    use crate::sync::materialize::{apply_inbound_payload, judged_as_row_of_batch, ApplyOutcome};
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let counter = user_with(&db, "q14receivedcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (session, line) = enter_count(&db, counter, article.rowid, location.rowid, 5.0).await;
    let line_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![line.id.into()],
    )
    .await;
    let counter_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![counter.into()],
    )
    .await;
    // The same count of another location of the session, as another PC sends it.
    let other = second_location(&db, &mut factory, article.rowid, 10.0).await;
    let other_location_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM stock_locations WHERE id = ?",
        vec![other.rowid.into()],
    )
    .await;
    let other_warehouse_sync = text_of(
        &db,
        "SELECT w.entity_sync_id FROM warehouses w JOIN stock_locations l ON l.warehouse_id = w.id WHERE l.id = ?",
        vec![other.rowid.into()],
    )
    .await;
    let mut received = wire_row(&db, "inventory_count_lines", &line_sync).await;
    for (k, v) in [
        ("location_sync_id", serde_json::json!(other_location_sync)),
        ("warehouse_sync_id", serde_json::json!(other_warehouse_sync)),
        ("is_critical", serde_json::json!(0)),
        ("approved_by_sync_id", serde_json::json!(counter_sync)),
        ("approved_at", serde_json::json!("2026-10-25T09:00:00Z")),
        ("approval_note", serde_json::json!("Recompté avec le magasinier")),
        ("self_approved", serde_json::json!(0)),
    ] {
        received.insert(k.into(), v);
    }
    let fresh = crate::sync::identity::new_entity_sync_id();
    let json = serde_json::Value::Object(received.clone()).to_string();
    let outcome = judged_as_row_of_batch(
        "snapshot-q14",
        apply_inbound_payload(&db, "inventory_count_lines", &fresh, "upsert", 1, &json),
    )
    .await
    .expect("apply");
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT is_critical * 10 + self_approved FROM inventory_count_lines WHERE entity_sync_id = ?",
            vec![fresh.clone().into()],
        )
        .await,
        10,
        "stored above the tolerance (judged here); its self-approval flag kept as received"
    );
    let listed = crate::inventory::controls::list_count_lines(&db, session.id)
        .await
        .expect("lines")
        .into_iter()
        .find(|l| l.location_id == other.rowid)
        .expect("the received line");
    assert_eq!(
        (listed.is_critical, listed.self_approved),
        (1, 1),
        "above the tolerance here, approved by its counter: an auto-approbation"
    );
    // The sender's flag stays 0 in its later versions: raised here from it, that is no lowering of
    // the flag held here (the version is judged as when it arrived).
    inbox_put(
        &db,
        "snapshot-q14",
        "inventory_count_lines",
        &fresh,
        1,
        &received,
        "applied",
    )
    .await;
    assert_eq!(
        verdict_code(
            crate::inventory::count_four_eyes::inbound_count_line_check(&db, &fresh, &received)
                .await
                .expect("judged")
                .as_ref()
        ),
        "pass",
        "the sender's own flag, received again"
    );
}

/// A plant tolerance set on `db` only (capture suppressed: never sent), as an edit another PC has
/// not received yet.
async fn tolerance_here_only(db: &DatabaseConnection, key: &str, value: &str) {
    crate::sync::capture::begin_suppression(db).await.expect("suppress");
    crate::settings::set_setting(db, key, "tenant", value, None, "test")
        .await
        .expect("tolerance");
    crate::sync::capture::end_suppression(db).await.expect("unsuppress");
}

/// Review of b4f8b2fd (B1): a count line accepted here within the tolerance, approved by its
/// counter, is not judged again by a later version whose checked fields are unchanged — here the
/// version that carries its posting, after the tolerance was tightened on this PC. The version is
/// accepted, the count's movement is applied with it, no conflict.
#[tokio::test]
async fn a_line_accepted_within_the_tolerance_is_not_judged_again_by_its_posting() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    crate::settings::set_setting(
        &a,
        crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD,
        "tenant",
        "50",
        None,
        "test",
    )
    .await
    .expect("percent threshold");
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sites_made_locally(&a, &b, false, false).await;
    let counter = user_with(&a, "rejudgedcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let pair_b = local_pair(&b, &article, &location).await;
    // 7 counted of 10: 30 %, within 50 %: its counter approves it (no auto-approbation).
    let (session, line) = enter_count(&a, counter, article.rowid, location.rowid, 7.0).await;
    assert_eq!(line.is_critical, 0, "within the tolerance");
    let session = crate::inventory::controls::transition_count_session(&a, move_to(&session, "submitted", counter))
        .await
        .expect("submitted");
    let approved = crate::inventory::controls::approve_count_line(
        &a,
        crate::inventory::domain::ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: i64::from(counter),
            reviewer_evidence: "Bac vérifié au comptage".into(),
        },
    )
    .await
    .expect("approved by its counter");
    assert_eq!(approved.self_approved, 0);
    crate::inventory::controls::transition_count_session(&a, move_to(&session, "approved", counter))
        .await
        .expect("approved");
    let session_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert!(
        conflict_kinds(&b).await.is_empty(),
        "accepted on B within the tolerance"
    );

    // The tolerance tightened on B (10 %: the 30 % variance is above it now), then A posts.
    tolerance_here_only(&b, crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD, "10").await;
    post_on(&a, counter, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert!(
        conflict_kinds(&b).await.is_empty(),
        "the posting version is not judged again: no COUNT_SELF_APPROVAL"
    );
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (1, 1));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        7000,
        "the movement is applied with its line"
    );
    // Not judged again in what B stores either: within, no auto-approbation (a second approval
    // stays optional for it, Q15).
    let listed = crate::inventory::controls::list_count_lines(&b, session_by_sync_id(&b, &session_sync).await.id)
        .await
        .expect("lines")
        .into_iter()
        .next()
        .expect("the line");
    assert_eq!(
        (listed.is_critical, listed.self_approved),
        (0, 0),
        "kept within the tolerance as accepted"
    );
}

/// CMMS Advisor Q15: a count approved by its counter within the tolerance where it was posted, held
/// on a PC with a stricter tolerance (above it there), is settled by an optional second approval
/// given where it was posted: when that version arrives the held line and its movement are applied
/// together and the conflict closes.
#[tokio::test]
async fn an_optional_second_approval_releases_a_count_held_by_a_stricter_tolerance() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    crate::settings::set_setting(
        &a,
        crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD,
        "tenant",
        "50",
        None,
        "test",
    )
    .await
    .expect("percent threshold");
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sites_made_locally(&a, &b, false, false).await;
    // Stricter on B (10 %), not yet received by A.
    tolerance_here_only(&b, crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD, "10").await;
    let counter = user_with(&a, "optionalsecondcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let pair_b = local_pair(&b, &article, &location).await;
    // 7 counted of 10 on A: 30 %, within 50 %: its counter approves it, A posts it.
    let (session, line) = enter_count(&a, counter, article.rowid, location.rowid, 7.0).await;
    assert_eq!(line.is_critical, 0, "within the tolerance on A");
    let session = crate::inventory::controls::transition_count_session(&a, move_to(&session, "submitted", counter))
        .await
        .expect("submitted");
    let approved = crate::inventory::controls::approve_count_line(
        &a,
        crate::inventory::domain::ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: i64::from(counter),
            reviewer_evidence: "Bac vérifié au comptage".into(),
        },
    )
    .await
    .expect("approved by its counter");
    assert_eq!(approved.self_approved, 0);
    crate::inventory::controls::transition_count_session(&a, move_to(&session, "approved", counter))
        .await
        .expect("approved");
    let session_sync = text_of(
        &a,
        "SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?",
        vec![session.id.into()],
    )
    .await;
    post_on(&a, counter, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        conflict_kinds(&b).await,
        vec!["inventory_count_lines:COUNT_SELF_APPROVAL".to_string()],
        "above the tolerance on B: held"
    );
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        10000,
        "held on B: the stock does not move"
    );

    // The optional second approval, on A where the count is posted.
    let posted = crate::inventory::controls::list_count_lines(&a, session.id)
        .await
        .expect("lines")
        .into_iter()
        .next()
        .expect("the line");
    assert_eq!(posted.is_critical, 0, "still within the tolerance on A");
    let approver = second_person(&a, counter).await;
    let seconded = crate::inventory::controls::second_approve_count_line(
        &a,
        crate::inventory::domain::SecondApproveInventoryCountLineInput {
            line_id: posted.id,
            expected_row_version: posted.row_version,
            reviewer_id: i64::from(approver),
            reviewer_evidence: "Comptage vérifié par un second".into(),
        },
    )
    .await
    .expect("optional second approval within the tolerance");
    assert_eq!(seconded.second_approved_by_id, Some(i64::from(approver)));

    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert!(conflict_kinds(&b).await.is_empty(), "the conflict closed by itself");
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (1, 1));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        7000,
        "released on B: the movement is applied with its line"
    );
}

/// Review of b4f8b2fd (S1, S2): a received version may clear the "above the tolerance" flag held
/// here without a new entry only when this PC raised that flag itself from a received "within"
/// and the line has not changed here since (its device-local mark). Never a flag entered here,
/// whatever the inbox holds; never once the last version applied here said "above" itself; and
/// the flag lowered that way is stored as this PC judges the line now.
#[tokio::test]
async fn only_a_flag_raised_here_from_a_received_within_may_be_cleared_without_a_new_entry() {
    use crate::inventory::count_four_eyes::{inbound_count_line_check, SETTING_PERCENT_THRESHOLD};
    use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let counter = user_with(&db, "raisedmarkcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    // Entered here, 5 of 10, tolerance blank: above (strict).
    let (session, local) = enter_count(&db, counter, article.rowid, location.rowid, 5.0).await;
    assert_eq!(local.is_critical, 1);
    let local_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![local.id.into()],
    )
    .await;
    let changed = crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_CHANGED;
    let judged = |sync_id: String, payload: serde_json::Map<String, serde_json::Value>| {
        let db = &db;
        async move {
            let verdict = inbound_count_line_check(db, &sync_id, &payload).await.expect("judged");
            verdict_code(verdict.as_ref())
        }
    };
    let with_critique = |mut p: serde_json::Map<String, serde_json::Value>, critique: i64| {
        p.insert("is_critical".into(), serde_json::json!(critique));
        p
    };

    // A flag entered here is never lowered, even with an applied version saying "within" in the
    // inbox (the rule before this fix took that as a raise of its own).
    let entered = wire_row(&db, "inventory_count_lines", &local_sync).await;
    inbox_put(
        &db,
        "exchange-raised",
        "inventory_count_lines",
        &local_sync,
        1,
        &with_critique(entered.clone(), 0),
        "applied",
    )
    .await;
    assert_eq!(
        judged(local_sync.clone(), with_critique(entered, 0)).await,
        changed,
        "entered here"
    );

    // A line received from another PC (another location of the session), not approved, sent as
    // within the tolerance: applied, raised here (blank tolerance: strict) and marked.
    let other = second_location(&db, &mut factory, article.rowid, 10.0).await;
    let mut received = wire_row(&db, "inventory_count_lines", &local_sync).await;
    for (k, v) in [
        (
            "location_sync_id",
            serde_json::json!(
                text_of(
                    &db,
                    "SELECT entity_sync_id FROM stock_locations WHERE id = ?",
                    vec![other.rowid.into()]
                )
                .await
            ),
        ),
        (
            "warehouse_sync_id",
            serde_json::json!(
                text_of(
                    &db,
                    "SELECT w.entity_sync_id FROM warehouses w JOIN stock_locations l ON l.warehouse_id = w.id
                     WHERE l.id = ?",
                    vec![other.rowid.into()]
                )
                .await
            ),
        ),
        ("is_critical", serde_json::json!(0)),
    ] {
        received.insert(k.into(), v);
    }
    let fresh = crate::sync::identity::new_entity_sync_id();
    let mut version = 1;
    let mut apply = |payload: serde_json::Map<String, serde_json::Value>| {
        version += 1;
        let (db, fresh, version) = (&db, fresh.clone(), version);
        async move {
            let json = serde_json::Value::Object(payload).to_string();
            apply_inbound_payload(db, "inventory_count_lines", &fresh, "upsert", version, &json)
                .await
                .expect("apply")
        }
    };
    let state = || {
        let (db, fresh) = (&db, fresh.clone());
        async move {
            scalar_i64(
                db,
                "SELECT is_critical * 10 + (critique_raised_version IS NOT NULL
                        AND critique_raised_version = row_version)
                 FROM inventory_count_lines WHERE entity_sync_id = ?",
                vec![fresh.into()],
            )
            .await
        }
    };
    let outcome = apply(received.clone()).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 11, "raised here from the received within, marked");
    let held = || {
        let (db, fresh) = (&db, fresh.clone());
        async move { wire_row(db, "inventory_count_lines", &fresh).await }
    };
    assert_eq!(
        held().await.get("critique_raised_version"),
        None,
        "the mark never travels"
    );

    // Its later version, still within: no lowering of a flag raised here; applied, raised again.
    assert_eq!(judged(fresh.clone(), with_critique(held().await, 0)).await, "pass");
    let outcome = apply(with_critique(held().await, 0)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 11, "still above here: raised again");

    // Entered again here (same quantity): the line changed here, its flag is an entry of its own.
    let session = session_by_code(&db, &session.session_code).await;
    let line_here = crate::inventory::controls::list_count_lines(&db, session.id)
        .await
        .expect("lines")
        .into_iter()
        .find(|l| l.location_id == other.rowid)
        .expect("the received line");
    crate::inventory::controls::upsert_count_line(
        &db,
        crate::inventory::domain::UpsertInventoryCountLineInput {
            session_id: session.id,
            article_id: article.rowid,
            location_id: other.rowid,
            counted_qty: 5.0,
            variance_reason_code: Some("COUNT_ERROR".into()),
            expected_row_version: Some(line_here.row_version),
            actor_id: Some(i64::from(counter)),
        },
    )
    .await
    .expect("entered again");
    assert_eq!(state().await, 10, "entered here: not marked");
    assert_eq!(
        judged(fresh.clone(), with_critique(held().await, 0)).await,
        changed,
        "entered again here"
    );

    // The last version applied here said "above" itself: the flag held is that one, not a raise.
    let outcome = apply(with_critique(held().await, 1)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 10, "received above: not marked");
    assert_eq!(
        judged(fresh.clone(), with_critique(held().await, 0)).await,
        changed,
        "the last applied version said above"
    );

    // A new entry received within: raised here again and marked. Then the tolerance loosened (70 %:
    // the 60 % variance is within it now): the next version within, unchanged, is accepted (the
    // flag raised here, no lowering) and not judged again — the flag held stays raised and marked;
    // only a new entry is judged with the tolerance of now, stored within.
    let mut entry = with_critique(held().await, 0);
    entry.insert("counted_qty".into(), serde_json::json!(4.0));
    entry.insert("variance_qty".into(), serde_json::json!(-6.0));
    let outcome = apply(entry).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 11, "a new entry, raised here");
    tolerance_here_only(&db, SETTING_PERCENT_THRESHOLD, "70").await;
    assert_eq!(judged(fresh.clone(), with_critique(held().await, 0)).await, "pass");
    let outcome = apply(with_critique(held().await, 0)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 11, "unchanged: kept as judged when it arrived");
    let mut entry = with_critique(held().await, 0);
    entry.insert("counted_qty".into(), serde_json::json!(5.0));
    entry.insert("variance_qty".into(), serde_json::json!(-5.0));
    let outcome = apply(entry).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 0, "a new entry within here now: stored within, no mark");
}

/// Review of 5ead90e8 (gap): the one path where a received version lowers the flag held here
/// without a new entry — a flag this PC raised itself (marked), the same quantities, another
/// approval, judged within the tolerance of now: stored within, the mark cleared.
#[tokio::test]
async fn a_flag_raised_here_is_lowered_by_a_changed_version_judged_within_now() {
    use crate::inventory::count_four_eyes::{inbound_count_line_check, SETTING_PERCENT_THRESHOLD};
    use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let counter = user_with(&db, "loweredmarkcounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let (_, local) = enter_count(&db, counter, article.rowid, location.rowid, 5.0).await;
    let local_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![local.id.into()],
    )
    .await;
    // A line of the same session received from another PC (another location), 5 of 10, sent as
    // within; the tolerance here is blank (strict): raised here and marked.
    let other = second_location(&db, &mut factory, article.rowid, 10.0).await;
    let mut received = wire_row(&db, "inventory_count_lines", &local_sync).await;
    received.insert(
        "location_sync_id".into(),
        serde_json::json!(
            text_of(
                &db,
                "SELECT entity_sync_id FROM stock_locations WHERE id = ?",
                vec![other.rowid.into()]
            )
            .await
        ),
    );
    received.insert(
        "warehouse_sync_id".into(),
        serde_json::json!(
            text_of(
                &db,
                "SELECT w.entity_sync_id FROM warehouses w JOIN stock_locations l ON l.warehouse_id = w.id
                 WHERE l.id = ?",
                vec![other.rowid.into()]
            )
            .await
        ),
    );
    received.insert("is_critical".into(), serde_json::json!(0));
    let fresh = crate::sync::identity::new_entity_sync_id();
    let apply = |payload: serde_json::Map<String, serde_json::Value>, version: i64| {
        let (db, fresh) = (&db, fresh.clone());
        async move {
            let json = serde_json::Value::Object(payload).to_string();
            apply_inbound_payload(db, "inventory_count_lines", &fresh, "upsert", version, &json)
                .await
                .expect("apply")
        }
    };
    let state = || {
        let (db, fresh) = (&db, fresh.clone());
        async move {
            scalar_i64(
                db,
                "SELECT is_critical * 10 + (critique_raised_version IS NOT NULL
                        AND critique_raised_version = row_version)
                 FROM inventory_count_lines WHERE entity_sync_id = ?",
                vec![fresh.into()],
            )
            .await
        }
    };
    let outcome = apply(received, 2).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 11, "raised here, marked");

    // The tolerance loosened here (70 %: the 50 % variance is within it now); a version with the
    // same quantities, still "within", now approved by another person: a changed version, judged
    // again — within — and the flag this PC raised is lowered, its mark cleared.
    tolerance_here_only(&db, SETTING_PERCENT_THRESHOLD, "70").await;
    let approver = second_person(&db, counter).await;
    let approver_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![approver.into()],
    )
    .await;
    let mut approved = wire_row(&db, "inventory_count_lines", &fresh).await;
    for (k, v) in [
        ("is_critical", serde_json::json!(0)),
        ("approved_by_sync_id", serde_json::json!(approver_sync)),
        ("approved_at", serde_json::json!("2026-10-25T09:00:00Z")),
        ("approval_note", serde_json::json!("Écart vérifié par un second")),
    ] {
        approved.insert(k.into(), v);
    }
    let verdict = inbound_count_line_check(&db, &fresh, &approved).await.expect("judged");
    assert_eq!(
        verdict_code(verdict.as_ref()),
        "pass",
        "the lowering of a flag raised here"
    );
    let outcome = apply(approved, 3).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state().await, 0, "judged within now: stored within, no mark");
}

/// Review of 5ead90e8 (S1, N1): a received version of a line held here is "already judged" only
/// under the same count, article and location (whose cost judges it) and the same "approval
/// required"; otherwise it is judged again. A line accepted within the tolerance, approved by its
/// counter, re-sent under a costlier location is judged there — above: refused — not accepted
/// unchanged.
#[tokio::test]
async fn a_line_held_within_re_sent_under_another_location_is_judged_again() {
    use crate::inventory::count_four_eyes::{held_judgement, inbound_count_line_check, SYNC_SELF_APPROVAL};
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&db, &mut factory, 10.0).await;
    let costly = second_location(&db, &mut factory, article.rowid, 10.0).await;
    for (place, cost) in [(location.rowid, 1.0), (costly.rowid, 100.0)] {
        exec(
            &db,
            "UPDATE stock_balances SET moving_avg_unit_cost = ? WHERE article_id = ? AND location_id = ?",
            vec![cost.into(), article.rowid.into(), place.into()],
        )
        .await;
    }
    crate::settings::set_setting(
        &db,
        crate::inventory::count_four_eyes::SETTING_VALUE_THRESHOLD,
        "tenant",
        "50",
        None,
        "test",
    )
    .await
    .expect("value threshold");
    let counter = user_with(&db, "movedlinecounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    // 5 of 10 at a cost of 1: a value of 5, within 50; its counter approves it.
    let (session, line) = enter_count(&db, counter, article.rowid, location.rowid, 5.0).await;
    assert_eq!(line.is_critical, 0, "within the tolerance");
    crate::inventory::controls::transition_count_session(&db, move_to(&session, "submitted", counter))
        .await
        .expect("submitted");
    let approved = crate::inventory::controls::approve_count_line(
        &db,
        crate::inventory::domain::ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: i64::from(counter),
            reviewer_evidence: "Bac vérifié au comptage".into(),
        },
    )
    .await
    .expect("approved by its counter");
    assert_eq!((approved.is_critical, approved.self_approved), (0, 0));
    let line_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM inventory_count_lines WHERE id = ?",
        vec![line.id.into()],
    )
    .await;
    let held = wire_row(&db, "inventory_count_lines", &line_sync).await;
    let changed = |key: &str, value: serde_json::Value| {
        let mut p = held.clone();
        p.insert(key.into(), value);
        p
    };
    let costly_sync = text_of(
        &db,
        "SELECT entity_sync_id FROM stock_locations WHERE id = ?",
        vec![costly.rowid.into()],
    )
    .await;

    // The same version: already judged (the flag held, within, is kept).
    assert_eq!(held_judgement(&db, &line_sync, &held).await.expect("held"), Some(false));
    let verdict = inbound_count_line_check(&db, &line_sync, &held).await.expect("judged");
    assert_eq!(verdict_code(verdict.as_ref()), "pass");
    // Another count, article, location or "approval required": judged again.
    for (key, value) in [
        (
            "session_sync_id",
            serde_json::json!(crate::sync::identity::new_entity_sync_id()),
        ),
        (
            "article_sync_id",
            serde_json::json!(crate::sync::identity::new_entity_sync_id()),
        ),
        ("location_sync_id", serde_json::json!(costly_sync)),
        ("approval_required", serde_json::json!(0)),
    ] {
        assert_eq!(
            held_judgement(&db, &line_sync, &changed(key, value))
                .await
                .expect("held"),
            None,
            "{key} changed: not already judged"
        );
    }
    // Under the costlier location (5 × 100 = 500, above 50), approved by its counter without a
    // self-approval: refused, not accepted as the line held.
    let verdict = inbound_count_line_check(
        &db,
        &line_sync,
        &changed("location_sync_id", serde_json::json!(costly_sync)),
    )
    .await
    .expect("judged");
    assert_eq!(
        verdict_code(verdict.as_ref()),
        SYNC_SELF_APPROVAL,
        "judged at the costlier location"
    );
}

/// A count within the tolerance approved by its counter on A (accepted on B), then posted on A:
/// its adjustment (−3, of 10 counted 7) is on the server, not sent to B yet ([`forged_adjustment`]:
/// with another quantity, −8).
struct ForgedAdjustment {
    a: DatabaseConnection,
    b: DatabaseConnection,
    server: TestServer,
    pair_b: (i64, i64),
    counter: i32,
    session_sync: String,
    line_sync: String,
    adjustment: String,
    /// The adjustment as A posted it.
    real: serde_json::Map<String, serde_json::Value>,
    other_location: CreatedRow,
}

/// The quantity of a row on the server changed (a record forged or corrupted on its way).
fn forge_quantity(server: &mut TestServer, entity_type: &str, sync_id: &str, quantity: f64) {
    let row = server
        .state
        .get_mut(&(entity_type.to_string(), sync_id.to_string()))
        .expect("the row is on the server");
    let mut payload: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&row.payload_json).expect("payload");
    payload.insert("quantity".into(), serde_json::json!(quantity));
    row.payload_json = serde_json::to_string(&payload).expect("json");
    row.payload_hash = crate::sync::queries::hash_payload(&row.payload_json);
}

async fn forged_adjustment(name: &str) -> ForgedAdjustment {
    let mut posted = posted_count(name).await;
    forge_quantity(&mut posted.server, "inventory_transactions", &posted.adjustment, -8.0);
    posted
}

async fn posted_count(name: &str) -> ForgedAdjustment {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    crate::settings::set_setting(
        &a,
        crate::inventory::count_four_eyes::SETTING_PERCENT_THRESHOLD,
        "tenant",
        "50",
        None,
        "test",
    )
    .await
    .expect("percent threshold");
    let other_location = second_location(&a, &mut factory, article.rowid, 10.0).await;
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sites_made_locally(&a, &b, false, false).await;
    let counter = user_with(&a, name, &[crate::rbac::permissions::INV_MANAGE]).await;
    let pair_b = local_pair(&b, &article, &location).await;
    // 7 counted of 10: 30 %, within 50 %: its counter approves it, accepted on B.
    let (session, line) = enter_count(&a, counter, article.rowid, location.rowid, 7.0).await;
    let session = crate::inventory::controls::transition_count_session(&a, move_to(&session, "submitted", counter))
        .await
        .expect("submitted");
    crate::inventory::controls::approve_count_line(
        &a,
        crate::inventory::domain::ApproveInventoryCountLineInput {
            line_id: line.id,
            expected_row_version: line.row_version,
            reviewer_id: i64::from(counter),
            reviewer_evidence: "Bac vérifié au comptage".into(),
        },
    )
    .await
    .expect("approved by its counter");
    crate::inventory::controls::transition_count_session(&a, move_to(&session, "approved", counter))
        .await
        .expect("approved");
    let session_sync = session_sync_of(&a, &session.session_code).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert!(
        conflict_kinds(&b).await.is_empty(),
        "accepted on B within the tolerance"
    );

    // A posts (−3, the line's variance).
    post_on(&a, counter, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    let adjustment = movement_sync_id(&a, &session_sync, "COUNT_ADJUST").await;
    let real = wire_row(&a, "inventory_transactions", &adjustment).await;
    assert_eq!(real.get("quantity"), Some(&serde_json::json!(-3.0)));
    let line_sync = line_sync_of(&a, &session.session_code).await;
    ForgedAdjustment {
        a,
        b,
        server,
        pair_b,
        counter,
        session_sync,
        line_sync,
        adjustment,
        real,
        other_location,
    }
}

/// On `db` the line naming the forged adjustment is one visible `COUNT_APPROVAL_RECORD_INVALID`
/// conflict saying why, and the adjustment is not applied: the stock (10) does not move.
async fn assert_forged_shown_and_held(db: &DatabaseConnection, forged: (&str, (i64, i64))) {
    let (session_sync, pair) = forged;
    assert_eq!(
        conflict_kinds(db).await,
        vec!["inventory_count_lines:COUNT_APPROVAL_RECORD_INVALID".to_string()],
        "a visible conflict, not a silent wait"
    );
    let detail = text_of(
        db,
        "SELECT detail_message FROM sync_conflicts WHERE conflict_type = 'COUNT_APPROVAL_RECORD_INVALID'
         ORDER BY id DESC LIMIT 1",
        vec![],
    )
    .await;
    assert!(
        detail.contains("as its posted adjustment")
            && detail.contains("its quantity (-8) is not the count's variance (-3)"),
        "{detail}"
    );
    assert_eq!(
        count_movements(db, session_sync, "COUNT_ADJUST").await,
        (0, 0),
        "not the line's adjustment: it is held"
    );
    assert_eq!(
        on_hand_milli(db, pair.0, pair.1).await,
        10_000,
        "the stock does not move"
    );
}

/// Review of 5ead90e8 (S2): a received count adjustment is released by the line version naming it
/// only when it is that line's own adjustment — the line's count session, article and location,
/// of the line's variance — as a reversal must be the reversal of its count. Otherwise it is held
/// and the stock does not move; the line naming it is a visible COUNT_APPROVAL_RECORD_INVALID
/// conflict (review of d6562b88, §5). The count's reversal does not release it, "take theirs" is
/// refused and "keep mine" leaves it held.
#[tokio::test]
async fn a_count_adjustment_is_released_only_if_it_is_the_one_of_its_line() {
    use crate::inventory::count_four_eyes::inbound_count_movement_hold;
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        counter,
        session_sync,
        adjustment,
        real,
        other_location,
        ..
    } = forged_adjustment("ownadjustcounter").await;
    // The adjustment and the line version naming it arrive together.
    sync_applied(&mut server, "B", &b).await;
    assert_forged_shown_and_held(&b, (&session_sync, pair_b)).await;

    // Judged against the line version naming it, waiting on B: its own adjustment passes; another
    // quantity, article, location or count waits.
    let hold = |payload: serde_json::Map<String, serde_json::Value>| {
        let (b, adjustment) = (&b, adjustment.clone());
        async move {
            let verdict = inbound_count_movement_hold(b, &adjustment, &payload)
                .await
                .expect("judged");
            verdict_code(verdict.as_ref())
        }
    };
    // Review of 2faa97d5 (S1): even its own adjustment waits while the line version naming it is
    // refused (an open COUNT_APPROVAL_RECORD_INVALID conflict): never released while the line stays
    // unapplied, its conflict saying the stock did not move.
    match inbound_count_movement_hold(&b, &adjustment, &real)
        .await
        .expect("judged")
    {
        Some(crate::settings::site::InboundVerdict::Defer(why)) => assert!(
            why.contains("waits for the conflict of its count line") && why.contains("COUNT_APPROVAL_RECORD_INVALID"),
            "{why}"
        ),
        other => panic!("the line's own adjustment waits for the line's conflict: {other:?}"),
    }
    for (key, value) in [
        ("quantity", serde_json::json!(-8.0)),
        (
            "article_sync_id",
            serde_json::json!(crate::sync::identity::new_entity_sync_id()),
        ),
        ("location_sync_id", serde_json::json!(other_location.entity_sync_id)),
        (
            "source_sync_id",
            serde_json::json!(crate::sync::identity::new_entity_sync_id()),
        ),
    ] {
        let mut tampered = real.clone();
        tampered.insert(key.into(), value);
        assert_eq!(hold(tampered).await, "wait", "{key} is not the line's");
    }

    // The count reversed on A: its line version naming the forged adjustment is refused too, and
    // the reversal is never applied on its own.
    reverse_on(&a, second_person(&a, counter).await, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (0, 0));
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_REVERSAL").await,
        (0, 0),
        "the reversal of a held adjustment is held with it"
    );
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
    let kinds = conflict_kinds(&b).await;
    assert!(
        !kinds.is_empty()
            && kinds
                .iter()
                .all(|k| k == "inventory_count_lines:COUNT_APPROVAL_RECORD_INVALID"),
        "{kinds:?}"
    );

    // "Take theirs" judges the version again: refused. "Keep mine" sets it aside: the adjustment
    // and its reversal still wait, the stock does not move.
    use sea_orm::ConnectionTrait;
    let open = b
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT id, row_version FROM sync_conflicts
             WHERE conflict_type = 'COUNT_APPROVAL_RECORD_INVALID' AND status IN ('new', 'triaged', 'escalated')
             ORDER BY id DESC LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("query")
        .expect("conflict");
    let (conflict_id, version): (i64, i64) = (
        open.try_get("", "id").expect("id"),
        open.try_get("", "row_version").expect("version"),
    );
    let admin_b = i64::from(second_person(&b, counter).await);
    let resolve = |action: &'static str| {
        crate::sync::queries::resolve_sync_conflict(
            &b,
            admin_b,
            crate::sync::domain::ResolveSyncConflictInput {
                conflict_id,
                expected_row_version: version,
                action: action.into(),
                resolution_note: Some("Choix fait sur ce poste".into()),
            },
        )
    };
    let refused = format!("{:?}", resolve("accept_remote").await.expect_err("take theirs"));
    assert!(
        refused.contains("GATE_SYNC_SERVER_VERSION_NOT_APPLIED") && refused.contains("COUNT_APPROVAL_RECORD_INVALID"),
        "{refused}"
    );
    resolve("accept_local").await.expect("keep mine");
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried again");
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (0, 0));
    assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (0, 0));
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
}

/// Review of d6562b88 (§5): whatever the order the forged adjustment and the line version naming it
/// arrive in — the adjustment first (B's exchange ends before that line version), or the line
/// first (it waits for its adjustment) — the line is a visible COUNT_APPROVAL_RECORD_INVALID
/// conflict once both are here, and the stock does not move.
#[tokio::test]
async fn a_forged_count_adjustment_is_a_visible_conflict_whatever_the_order_of_arrival() {
    // The adjustment first.
    let ForgedAdjustment {
        b,
        mut server,
        pair_b,
        session_sync,
        line_sync,
        ..
    } = forged_adjustment("forgedfirstcounter").await;
    server.hold_back("B", &line_sync);
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_ADJUST").await,
        (0, 0),
        "it waits for the line version naming it"
    );
    assert!(conflict_kinds(&b).await.is_empty(), "nothing to judge it against yet");
    server.release("B");
    sync_applied(&mut server, "B", &b).await;
    assert_forged_shown_and_held(&b, (&session_sync, pair_b)).await;

    // The line first.
    let ForgedAdjustment {
        b,
        server,
        pair_b,
        session_sync,
        line_sync,
        adjustment,
        ..
    } = forged_adjustment("forgedlatecounter").await;
    let on_server = |entity_type: &str, sync_id: &str| {
        let row = server
            .state
            .get(&(entity_type.to_string(), sync_id.to_string()))
            .expect("on the server");
        let payload: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&row.payload_json).expect("payload");
        (row.row_version, payload)
    };
    let (line_version, line_row) = on_server("inventory_count_lines", &line_sync);
    inbox_put(
        &b,
        "delta-line",
        "inventory_count_lines",
        &line_sync,
        line_version,
        &line_row,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried");
    assert!(
        conflict_kinds(&b).await.is_empty(),
        "its adjustment has not arrived: the line waits for it"
    );
    let (adjustment_version, adjustment_row) = on_server("inventory_transactions", &adjustment);
    inbox_put(
        &b,
        "delta-movement",
        "inventory_transactions",
        &adjustment,
        adjustment_version,
        &adjustment_row,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried");
    assert_forged_shown_and_held(&b, (&session_sync, pair_b)).await;
}

/// Review of 2faa97d5 (S1): a corrected version of a forged adjustment waits while the line version
/// naming it is refused, and is released once that line is settled where it is received — by "take
/// theirs" (the movement applied first, through every check) or by a later version of the line
/// judged clean (its older conflict closes): the stock then moves once, by the count's variance.
#[tokio::test]
async fn a_held_adjustment_waits_for_its_line_conflict_and_is_released_once_it_is_settled() {
    for settled_by_take_theirs in [true, false] {
        let ForgedAdjustment {
            b,
            mut server,
            pair_b,
            counter,
            session_sync,
            line_sync,
            adjustment,
            real,
            ..
        } = forged_adjustment(if settled_by_take_theirs {
            "settletakecounter"
        } else {
            "settlenewcounter"
        })
        .await;
        sync_applied(&mut server, "B", &b).await;
        assert_forged_shown_and_held(&b, (&session_sync, pair_b)).await;
        let version_of = |entity_type: &str, sync_id: &str| {
            server
                .state
                .get(&(entity_type.to_string(), sync_id.to_string()))
                .expect("on the server")
                .row_version
        };
        // The adjustment's corrected version (−3) arrives: still held with its refused line.
        inbox_put(
            &b,
            "delta-fix",
            "inventory_transactions",
            &adjustment,
            version_of("inventory_transactions", &adjustment) + 1,
            &real,
            "pending",
        )
        .await;
        crate::sync::materialize::materialize_inbox(&b, 200, true)
            .await
            .expect("tried");
        assert_eq!(
            count_movements(&b, &session_sync, "COUNT_ADJUST").await,
            (0, 0),
            "held while its line is refused"
        );
        assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
        assert_eq!(
            conflict_kinds(&b).await,
            vec!["inventory_count_lines:COUNT_APPROVAL_RECORD_INVALID".to_string()]
        );

        if settled_by_take_theirs {
            use sea_orm::ConnectionTrait;
            let open = b
                .query_one(sea_orm::Statement::from_string(
                    sea_orm::DbBackend::Sqlite,
                    "SELECT id, row_version FROM sync_conflicts WHERE conflict_type = 'COUNT_APPROVAL_RECORD_INVALID'"
                        .to_string(),
                ))
                .await
                .expect("query")
                .expect("conflict");
            crate::sync::queries::resolve_sync_conflict(
                &b,
                i64::from(second_person(&b, counter).await),
                crate::sync::domain::ResolveSyncConflictInput {
                    conflict_id: open.try_get("", "id").expect("id"),
                    expected_row_version: open.try_get("", "row_version").expect("version"),
                    action: "accept_remote".into(),
                    resolution_note: Some("Mouvement corrigé reçu depuis".into()),
                },
            )
            .await
            .expect("take theirs: the movement is now the line's own");
        } else {
            let line_row = server
                .state
                .get(&("inventory_count_lines".to_string(), line_sync.clone()))
                .expect("on the server");
            let line_payload: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&line_row.payload_json).expect("payload");
            inbox_put(
                &b,
                "delta-line",
                "inventory_count_lines",
                &line_sync,
                line_row.row_version + 1,
                &line_payload,
                "pending",
            )
            .await;
            crate::sync::materialize::materialize_inbox(&b, 200, true)
                .await
                .expect("tried");
        }
        assert_eq!(
            count_movements(&b, &session_sync, "COUNT_ADJUST").await,
            (1, 1),
            "released once its line is settled"
        );
        assert_eq!(
            on_hand_milli(&b, pair_b.0, pair_b.1).await,
            7_000,
            "the count's variance"
        );
        assert!(
            conflict_kinds(&b).await.is_empty(),
            "the line's conflict is closed (take theirs: {settled_by_take_theirs})"
        );
    }
}

/// Review of 2faa97d5 (N1): once a count's adjustment is applied here, a received reversal of that
/// count is still matched against the line version naming it — a reversal of another quantity
/// (the line then a visible COUNT_APPROVAL_RECORD_INVALID conflict), or one under another id that
/// no line names, is held and the stock does not move. The real reversal is applied (back to 10).
#[tokio::test]
async fn a_reversal_of_a_count_applied_here_must_undo_its_lines_adjustment() {
    for forged in [true, false] {
        let ForgedAdjustment {
            a,
            b,
            mut server,
            pair_b,
            counter,
            session_sync,
            line_sync,
            ..
        } = posted_count(if forged {
            "forgedreversalcounter"
        } else {
            "realreversalcounter"
        })
        .await;
        sync_applied(&mut server, "B", &b).await;
        assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000, "released on B");
        assert!(conflict_kinds(&b).await.is_empty());

        reverse_on(&a, second_person(&a, counter).await, &session_sync).await;
        sync_applied(&mut server, "A", &a).await;
        let reversal = movement_sync_id(&a, &session_sync, "COUNT_REVERSAL").await;
        if forged {
            // A reversal of this count under another id, which no line names: held, unmatched.
            let mut other = wire_row(&a, "inventory_transactions", &reversal).await;
            other.insert("quantity".into(), serde_json::json!(8.0));
            let other_sync = crate::sync::identity::new_entity_sync_id();
            inbox_put(
                &b,
                "delta-other",
                "inventory_transactions",
                &other_sync,
                1,
                &other,
                "pending",
            )
            .await;
            // The reversal its line names, received with another quantity (+8 for +3).
            forge_quantity(&mut server, "inventory_transactions", &reversal, 8.0);
        }
        sync_applied(&mut server, "B", &b).await;
        if forged {
            assert_eq!(
                count_movements(&b, &session_sync, "COUNT_REVERSAL").await,
                (0, 0),
                "never applied unmatched"
            );
            assert_eq!(
                on_hand_milli(&b, pair_b.0, pair_b.1).await,
                7_000,
                "the stock does not move"
            );
            assert_eq!(
                conflict_kinds(&b).await,
                vec!["inventory_count_lines:COUNT_APPROVAL_RECORD_INVALID".to_string()]
            );
            let detail = text_of(
                &b,
                "SELECT detail_message FROM sync_conflicts WHERE entity_sync_id = ?",
                vec![line_sync.clone().into()],
            )
            .await;
            assert!(detail.contains("does not undo its posted adjustment"), "{detail}");
        } else {
            assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (1, 1));
            assert_eq!(
                on_hand_milli(&b, pair_b.0, pair_b.1).await,
                10_000,
                "the real reversal is applied"
            );
            assert!(conflict_kinds(&b).await.is_empty());
        }
    }
}

/// The open conflict of `db` on `entity_sync_id`: `(id, row_version)`.
async fn open_conflict_on(db: &DatabaseConnection, entity_sync_id: &str) -> (i64, i64) {
    (
        scalar_i64(
            db,
            "SELECT id FROM sync_conflicts WHERE entity_sync_id = ? AND status IN ('new', 'triaged', 'escalated')
             ORDER BY id DESC LIMIT 1",
            vec![entity_sync_id.into()],
        )
        .await,
        scalar_i64(
            db,
            "SELECT row_version FROM sync_conflicts WHERE entity_sync_id = ?
               AND status IN ('new', 'triaged', 'escalated') ORDER BY id DESC LIMIT 1",
            vec![entity_sync_id.into()],
        )
        .await,
    )
}

/// "Take theirs" on conflict `(id, row_version)` of `db`, by `user`.
async fn take_theirs(db: &DatabaseConnection, user: i32, conflict: (i64, i64)) -> crate::errors::AppResult<()> {
    resolve_conflict(db, user, conflict, "accept_remote").await
}

/// Review of 07d895a9 (B1): a later version of a count adjustment already applied — on B, received
/// from A, and on A, where it was posted and acknowledged — that changes its quantity is never
/// adopted: it would move the stock again by the difference with no line or four-eyes behind it.
/// It is a visible COUNT_APPROVAL_RECORD_INVALID conflict on the movement, the movement held keeps
/// its quantity, and "take theirs" is refused (judged again).
#[tokio::test]
async fn a_later_version_of_a_count_movement_applied_here_never_moves_the_stock_again() {
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        counter,
        adjustment,
        real,
        ..
    } = posted_count("newerforgedcounter").await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000, "released on B");
    let of_movement = |column: &'static str| {
        let (a, adjustment) = (&a, adjustment.clone());
        async move {
            scalar_i64(
                a,
                &format!("SELECT {column} FROM inventory_transactions WHERE entity_sync_id = ?"),
                vec![adjustment.into()],
            )
            .await
        }
    };
    let pair_a = (of_movement("article_id").await, of_movement("location_id").await);
    let version = server
        .state
        .get(&("inventory_transactions".to_string(), adjustment.clone()))
        .expect("on the server")
        .row_version;
    let mut forged = real.clone();
    forged.insert("quantity".into(), serde_json::json!(-8.0));
    for (pc, db, pair) in [("A", &a, pair_a), ("B", &b, pair_b)] {
        inbox_put(
            db,
            "delta-newer",
            "inventory_transactions",
            &adjustment,
            version + 1,
            &forged,
            "pending",
        )
        .await;
        crate::sync::materialize::materialize_inbox(db, 200, true)
            .await
            .expect("tried");
        assert_eq!(
            on_hand_milli(db, pair.0, pair.1).await,
            7_000,
            "{pc}: the stock does not move again"
        );
        assert_eq!(
            wire_row(db, "inventory_transactions", &adjustment)
                .await
                .get("quantity")
                .and_then(serde_json::Value::as_f64),
            Some(-3.0),
            "{pc}: the movement held keeps its quantity"
        );
        assert_eq!(
            conflict_kinds(db).await,
            vec!["inventory_transactions:COUNT_APPROVAL_RECORD_INVALID".to_string()],
            "{pc}: a visible conflict"
        );
        let detail = text_of(
            db,
            "SELECT detail_message FROM sync_conflicts WHERE entity_sync_id = ?",
            vec![adjustment.clone().into()],
        )
        .await;
        assert!(
            detail.contains("count line") && detail.contains("would move the stock by the difference"),
            "{pc}: {detail}"
        );
    }
    let refused = format!(
        "{:?}",
        take_theirs(
            &b,
            second_person(&b, counter).await,
            open_conflict_on(&b, &adjustment).await
        )
        .await
        .expect_err("take theirs is judged again")
    );
    assert!(
        refused.contains("GATE_SYNC_SERVER_VERSION_NOT_APPLIED") && refused.contains("would move the stock"),
        "{refused}"
    );
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000);

    // Review of ab58cd53: "keep mine" sends the movement as held here again (an upsert with its
    // quantity, never a delete of the history row); the stock does not move and the conflict closes.
    resolve_conflict(
        &b,
        second_person(&b, counter).await,
        open_conflict_on(&b, &adjustment).await,
        "accept_local",
    )
    .await
    .expect("keep mine");
    let staged = text_of(
        &b,
        "SELECT operation || ':' || payload_json FROM sync_outbox
         WHERE entity_type = 'inventory_transactions' AND entity_sync_id = ? AND status = 'pending'
         ORDER BY id DESC LIMIT 1",
        vec![adjustment.clone().into()],
    )
    .await;
    let (operation, payload) = staged.split_once(':').expect("operation:payload");
    assert_eq!(operation, "upsert", "{staged}");
    let payload: serde_json::Map<String, serde_json::Value> = serde_json::from_str(payload).expect("payload");
    assert_eq!(payload.get("quantity").and_then(serde_json::Value::as_f64), Some(-3.0));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        7_000,
        "keep mine moves nothing"
    );
    assert!(!conflict_kinds(&b)
        .await
        .contains(&"inventory_transactions:COUNT_APPROVAL_RECORD_INVALID".to_string()));
}

/// The person's `action` on conflict `(id, row_version)` of `db`, by `user`.
async fn resolve_conflict(
    db: &DatabaseConnection,
    user: i32,
    conflict: (i64, i64),
    action: &str,
) -> crate::errors::AppResult<()> {
    crate::sync::queries::resolve_sync_conflict(
        db,
        i64::from(user),
        crate::sync::domain::ResolveSyncConflictInput {
            conflict_id: conflict.0,
            expected_row_version: conflict.1,
            action: action.into(),
            resolution_note: Some("Décision prise après examen".into()),
        },
    )
    .await
    .map(|_| ())
}

/// Review of 07d895a9 (N1): an older version of a history row than the one applied here is not
/// written, and its inbox row keeps the reason (with the received record) as evidence.
#[tokio::test]
async fn an_older_version_of_a_count_movement_is_skipped_with_its_reason_kept() {
    let ForgedAdjustment {
        b,
        mut server,
        pair_b,
        adjustment,
        real,
        ..
    } = posted_count("olderversioncounter").await;
    sync_applied(&mut server, "B", &b).await;
    let applied = scalar_i64(
        &b,
        "SELECT synced_row_version FROM sync_entity_state WHERE entity_type = 'inventory_transactions'
           AND entity_sync_id = ?",
        vec![adjustment.clone().into()],
    )
    .await;
    let mut forged = real.clone();
    forged.insert("quantity".into(), serde_json::json!(-8.0));
    inbox_put(
        &b,
        "delta-older",
        "inventory_transactions",
        &adjustment,
        applied - 1,
        &forged,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried");
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000, "not written");
    assert_eq!(
        wire_row(&b, "inventory_transactions", &adjustment)
            .await
            .get("quantity")
            .and_then(serde_json::Value::as_f64),
        Some(-3.0)
    );
    assert!(conflict_kinds(&b).await.is_empty());
    let kept = text_of(
        &b,
        "SELECT apply_status || ':' || rejection_code || ':' || rejection_message FROM sync_inbox
         WHERE server_batch_id = 'delta-older'",
        vec![],
    )
    .await;
    assert!(
        kept.starts_with(&format!("applied:{}:", crate::sync::materialize::SKIPPED_OLDER_VERSION))
            && kept.contains(&format!("older than v{applied} applied here")),
        "{kept}"
    );
}

/// Review of 07d895a9 (S1): once a count's adjustment is applied here, a reversal under another id
/// that undoes it exactly, which no line names, is not applied (it waits for a line version naming
/// it); the count's real reversal, with its line, is. A second reversal of a count already
/// reversed here, under the count's own id, is a visible conflict, never applied.
#[tokio::test]
async fn an_exact_undo_reversal_no_line_names_is_never_applied() {
    use crate::inventory::count_four_eyes::inbound_count_movement_hold;
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        counter,
        session_sync,
        ..
    } = posted_count("exactundocounter").await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000);
    reverse_on(&a, second_person(&a, counter).await, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    let reversal = movement_sync_id(&a, &session_sync, "COUNT_REVERSAL").await;
    let real = wire_row(&a, "inventory_transactions", &reversal).await;
    assert_eq!(real.get("quantity").and_then(serde_json::Value::as_f64), Some(3.0));

    // The same +3 under another id, which no line names: it waits.
    let forged_sync = crate::sync::identity::new_entity_sync_id();
    inbox_put(
        &b,
        "delta-forged",
        "inventory_transactions",
        &forged_sync,
        1,
        &real,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried");
    assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (0, 0));
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        7_000,
        "the stock does not move"
    );
    let waiting = || {
        let (b, forged_sync) = (&b, forged_sync.clone());
        async move {
            text_of(
                b,
                "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = ?",
                vec![forged_sync.into()],
            )
            .await
        }
    };
    assert_eq!(waiting().await, "deferred");

    // The real reversal and its line: applied once, back to 10; the other one still waits.
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (1, 1));
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
    assert!(conflict_kinds(&b).await.is_empty());
    assert_eq!(waiting().await, "deferred", "never applied");

    // The count already reversed here (by a reversal now under another id): its own reversal again
    // is matched against the ledger and refused for a person.
    exec(
        &b,
        "UPDATE inventory_transactions SET entity_sync_id = ? WHERE entity_sync_id = ?",
        vec![
            crate::sync::identity::new_entity_sync_id().into(),
            reversal.clone().into(),
        ],
    )
    .await;
    match inbound_count_movement_hold(&b, &reversal, &real).await.expect("judged") {
        Some(crate::settings::site::InboundVerdict::Conflict { code, message }) => {
            assert_eq!(code, "COUNT_APPROVAL_RECORD_INVALID");
            assert!(message.contains("this count is already reversed here"), "{message}");
        }
        other => panic!("a second reversal of the count is refused: {other:?}"),
    }
}

/// Review of ab58cd53 (B2): once a count is reversed here, a reversal of it under a new id (the
/// same +3), named by a later version of its line, is never applied: it would reverse the count a
/// second time. It is a visible COUNT_APPROVAL_RECORD_INVALID conflict on that reversal and the
/// stock stays at 10. (S-a) That reversal is not held here: "keep mine" (it would send a delete of
/// a history row) and "take theirs" are refused, only "dismiss" or "escalate" close it.
#[tokio::test]
async fn a_count_reversed_here_is_never_reversed_again_by_a_line_version_naming_another_reversal() {
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        counter,
        session_sync,
        line_sync,
        ..
    } = posted_count("secondreversalcounter").await;
    sync_applied(&mut server, "B", &b).await;
    reverse_on(&a, second_person(&a, counter).await, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000, "the real reversal");
    assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (1, 1));
    assert!(conflict_kinds(&b).await.is_empty());
    let reversal = movement_sync_id(&a, &session_sync, "COUNT_REVERSAL").await;
    let real = wire_row(&b, "inventory_transactions", &reversal).await;

    // The same +3 under a new id, and a later version of the line naming it.
    let second = crate::sync::identity::new_entity_sync_id();
    let mut line = wire_row(&b, "inventory_count_lines", &line_sync).await;
    assert_eq!(
        line.get("reversed_transaction_sync_id"),
        Some(&serde_json::json!(reversal))
    );
    line.insert("reversed_transaction_sync_id".into(), serde_json::json!(second));
    let version = server
        .state
        .get(&("inventory_count_lines".to_string(), line_sync.clone()))
        .expect("on the server")
        .row_version;
    inbox_put(
        &b,
        "delta-second",
        "inventory_transactions",
        &second,
        1,
        &real,
        "pending",
    )
    .await;
    inbox_put(
        &b,
        "delta-second",
        "inventory_count_lines",
        &line_sync,
        version + 1,
        &line,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried");
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_REVERSAL").await,
        (1, 1),
        "never reversed twice"
    );
    assert_eq!(
        on_hand_milli(&b, pair_b.0, pair_b.1).await,
        10_000,
        "the stock does not move"
    );
    assert_eq!(
        conflict_kinds(&b).await,
        vec!["inventory_transactions:COUNT_APPROVAL_RECORD_INVALID".to_string()],
        "a visible conflict on the second reversal"
    );
    let detail = text_of(
        &b,
        "SELECT detail_message FROM sync_conflicts WHERE entity_sync_id = ?",
        vec![second.clone().into()],
    )
    .await;
    assert!(
        detail.contains("reverse its count a second time") && detail.contains("this count is already reversed here"),
        "{detail}"
    );
    assert_eq!(
        wire_row(&b, "inventory_count_lines", &line_sync)
            .await
            .get("reversed_transaction_sync_id"),
        Some(&serde_json::json!(reversal)),
        "the line held keeps its real reversal"
    );

    // S-a: nothing to keep or take for a history row not held here.
    let user = second_person(&b, counter).await;
    for action in ["accept_local", "accept_remote", "retry_later"] {
        let refused = format!(
            "{:?}",
            resolve_conflict(&b, user, open_conflict_on(&b, &second).await, action)
                .await
                .expect_err("refused")
        );
        assert!(
            refused.contains("GATE_SYNC_CONFLICT_REVIEW_ONLY"),
            "{action}: {refused}"
        );
    }
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = ?",
            vec![second.clone().into()],
        )
        .await,
        0,
        "no delete of a history row is queued"
    );
    resolve_conflict(&b, user, open_conflict_on(&b, &second).await, "dismiss")
        .await
        .expect("dismissed with its reason");
    assert!(conflict_kinds(&b).await.is_empty());
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
}

/// `(location id on db, location sync id, its warehouse sync id)` of a location created on `a`.
async fn location_elsewhere(
    a: &DatabaseConnection,
    db: &DatabaseConnection,
    location: &CreatedRow,
) -> (i64, String, String) {
    let warehouse = text_of(
        a,
        "SELECT w.entity_sync_id FROM warehouses w JOIN stock_locations l ON l.warehouse_id = w.id WHERE l.id = ?",
        vec![location.rowid.into()],
    )
    .await;
    let here = scalar_i64(
        db,
        "SELECT id FROM stock_locations WHERE entity_sync_id = ?",
        vec![location.entity_sync_id.clone().into()],
    )
    .await;
    (here, location.entity_sync_id.clone(), warehouse)
}

/// Review of 5fdad581 (B3): `movement` (a count movement as received) copied under a new id at
/// another location, and a later version of count line `line_sync` (as `db` holds it) moved to
/// that location naming the copy under `key`, both received by `db` and tried. Returns the copy's id.
async fn push_moved_elsewhere(
    db: &DatabaseConnection,
    server: &TestServer,
    line_sync: &str,
    key: &str,
    movement: &serde_json::Map<String, serde_json::Value>,
    elsewhere: (&str, &str),
) -> String {
    let (location_sync, warehouse_sync) = elsewhere;
    let forged = crate::sync::identity::new_entity_sync_id();
    let moved = |payload: &serde_json::Map<String, serde_json::Value>| {
        let mut payload = payload.clone();
        payload.insert("location_sync_id".into(), serde_json::json!(location_sync));
        if payload.contains_key("warehouse_sync_id") {
            payload.insert("warehouse_sync_id".into(), serde_json::json!(warehouse_sync));
        }
        payload
    };
    let mut line = moved(&wire_row(db, "inventory_count_lines", line_sync).await);
    line.insert(key.into(), serde_json::json!(forged));
    let version = server
        .state
        .get(&("inventory_count_lines".to_string(), line_sync.to_string()))
        .expect("on the server")
        .row_version;
    inbox_put(
        db,
        "delta-elsewhere",
        "inventory_transactions",
        &forged,
        1,
        &moved(movement),
        "pending",
    )
    .await;
    inbox_put(
        db,
        "delta-elsewhere",
        "inventory_count_lines",
        line_sync,
        version + 1,
        &line,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(db, 200, true)
        .await
        .expect("tried");
    forged
}

/// On `db` the movement `forged` is the one open conflict, a visible COUNT_APPROVAL_RECORD_INVALID
/// whose text contains `why`, and it is not applied.
async fn assert_forged_movement_refused(db: &DatabaseConnection, forged: &str, why: &str) {
    assert_eq!(
        conflict_kinds(db).await,
        vec!["inventory_transactions:COUNT_APPROVAL_RECORD_INVALID".to_string()],
        "a visible conflict on the forged movement"
    );
    let detail = text_of(
        db,
        "SELECT detail_message FROM sync_conflicts WHERE entity_sync_id = ?",
        vec![forged.to_string().into()],
    )
    .await;
    assert!(
        detail.contains(why) && detail.contains("the stock does not move"),
        "{detail}"
    );
    assert_eq!(
        scalar_i64(
            db,
            "SELECT COUNT(*) FROM inventory_transactions WHERE entity_sync_id = ?",
            vec![forged.to_string().into()],
        )
        .await,
        0,
        "not applied"
    );
}

/// Review of 5fdad581 (B3): once a count is reversed here, a reversal under a new id at ANOTHER
/// location, named by a later version of its line moved to that location (still naming the real
/// adjustment as posted), is refused: the adjustment it names is of another location. A visible
/// conflict on that reversal; the stock does not move at either location.
#[tokio::test]
async fn a_count_reversed_here_is_never_reversed_again_at_another_location() {
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        counter,
        session_sync,
        line_sync,
        other_location,
        ..
    } = posted_count("elsewhereaftercounter").await;
    let (elsewhere_b, location_sync, warehouse_sync) = location_elsewhere(&a, &b, &other_location).await;
    sync_applied(&mut server, "B", &b).await;
    reverse_on(&a, second_person(&a, counter).await, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000, "the real reversal");
    assert_eq!(on_hand_milli(&b, pair_b.0, elsewhere_b).await, 10_000);
    assert!(conflict_kinds(&b).await.is_empty());
    let reversal = movement_sync_id(&a, &session_sync, "COUNT_REVERSAL").await;
    let real = wire_row(&b, "inventory_transactions", &reversal).await;

    let forged = push_moved_elsewhere(
        &b,
        &server,
        &line_sync,
        "reversed_transaction_sync_id",
        &real,
        (&location_sync, &warehouse_sync),
    )
    .await;
    assert_forged_movement_refused(&b, &forged, "not an adjustment of the same count").await;
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_REVERSAL").await,
        (1, 1),
        "never reversed twice"
    );
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
    assert_eq!(
        on_hand_milli(&b, pair_b.0, elsewhere_b).await,
        10_000,
        "the stock elsewhere does not move"
    );
    assert_eq!(
        wire_row(&b, "inventory_count_lines", &line_sync)
            .await
            .get("reversed_transaction_sync_id"),
        Some(&serde_json::json!(reversal)),
        "the line held keeps its real reversal"
    );
}

/// Review of 5fdad581 (B3), the other order: the reversal at another location arrives BEFORE the
/// real one. It is refused (a visible conflict), and the real reversal is still applied when it
/// arrives: the count is reversed once, at its own location.
#[tokio::test]
async fn a_reversal_at_another_location_before_the_real_one_is_refused() {
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        counter,
        session_sync,
        line_sync,
        other_location,
        ..
    } = posted_count("elsewherebeforecounter").await;
    let (elsewhere_b, location_sync, warehouse_sync) = location_elsewhere(&a, &b, &other_location).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000, "posted");
    // A reverses; B has not received it yet.
    reverse_on(&a, second_person(&a, counter).await, &session_sync).await;
    sync_applied(&mut server, "A", &a).await;
    let reversal = movement_sync_id(&a, &session_sync, "COUNT_REVERSAL").await;
    let real = wire_row(&a, "inventory_transactions", &reversal).await;

    let forged = push_moved_elsewhere(
        &b,
        &server,
        &line_sync,
        "reversed_transaction_sync_id",
        &real,
        (&location_sync, &warehouse_sync),
    )
    .await;
    assert_forged_movement_refused(&b, &forged, "not an adjustment of the same count").await;
    assert_eq!(count_movements(&b, &session_sync, "COUNT_REVERSAL").await, (0, 0));
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000);
    assert_eq!(on_hand_milli(&b, pair_b.0, elsewhere_b).await, 10_000);

    // The real reversal arrives: applied once, at its own location.
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_REVERSAL").await,
        (1, 1),
        "reversed once"
    );
    assert_eq!(movement_sync_id(&b, &session_sync, "COUNT_REVERSAL").await, reversal);
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000, "the real reversal");
    assert_eq!(
        on_hand_milli(&b, pair_b.0, elsewhere_b).await,
        10_000,
        "the stock elsewhere does not move"
    );
    assert_eq!(
        conflict_kinds(&b).await,
        vec!["inventory_transactions:COUNT_APPROVAL_RECORD_INVALID".to_string()],
        "only the forged reversal stays for a person"
    );
}

/// Review of 5fdad581 (root-cause sweep): a count line posted here is never posted a second time
/// elsewhere. A later version of it moved to another location, naming a new adjustment there (its
/// line's variance, so it matches the version naming it), is refused: the line held here is of
/// another count. A visible conflict on that adjustment; the stock does not move at either location.
#[tokio::test]
async fn a_count_line_posted_here_is_never_posted_again_at_another_location() {
    let ForgedAdjustment {
        a,
        b,
        mut server,
        pair_b,
        session_sync,
        line_sync,
        adjustment,
        other_location,
        ..
    } = posted_count("elsewherepostcounter").await;
    let (elsewhere_b, location_sync, warehouse_sync) = location_elsewhere(&a, &b, &other_location).await;
    sync_applied(&mut server, "B", &b).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000, "posted");
    assert_eq!(on_hand_milli(&b, pair_b.0, elsewhere_b).await, 10_000);
    let real = wire_row(&b, "inventory_transactions", &adjustment).await;

    let forged = push_moved_elsewhere(
        &b,
        &server,
        &line_sync,
        "posted_transaction_sync_id",
        &real,
        (&location_sync, &warehouse_sync),
    )
    .await;
    assert_forged_movement_refused(&b, &forged, "post that line a second time").await;
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (1, 1));
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 7_000);
    assert_eq!(
        on_hand_milli(&b, pair_b.0, elsewhere_b).await,
        10_000,
        "the stock elsewhere does not move"
    );
    assert_eq!(
        wire_row(&b, "inventory_count_lines", &line_sync)
            .await
            .get("posted_transaction_sync_id"),
        Some(&serde_json::json!(adjustment)),
        "the line held keeps its posting"
    );
}

/// Review of 07d895a9 (S2): the same count posted on A (−3) and, offline, on B (−4) then reversed
/// on B (+4, from B's own posting). B's posting lost; its reversal (the count's own id) reaches A,
/// where no line names it and it undoes no posting held: it is not applied, and it is a visible
/// COUNT_APPROVAL_RECORD_INVALID conflict on A rather than a silent wait. Applying it would put A
/// at 11 where A counted 7 (and the PC that lost is itself off by its adoption after its reversal,
/// a separate issue): a person recounts.
#[tokio::test]
async fn a_reversal_of_a_divergent_posting_that_lost_is_a_conflict_where_it_undoes_nothing() {
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
    reverse_on(&b, users.1, &session_sync_id).await;
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
    for (pc, db) in [("A", &a), ("B", &b), ("A", &a), ("B", &b), ("A", &a)] {
        sync_applied(&mut server, pc, db).await;
    }
    assert_eq!(
        on_hand_milli(&a, pair_a.0, pair_a.1).await,
        7_000,
        "A: not moved by a reversal undoing no posting held here"
    );
    assert_eq!(count_movements(&a, &session_sync_id, "COUNT_REVERSAL").await, (0, 0));
    assert!(
        conflict_kinds(&a)
            .await
            .contains(&"inventory_transactions:COUNT_APPROVAL_RECORD_INVALID".to_string()),
        "A: a visible conflict, not a silent wait: {:?}",
        conflict_kinds(&a).await
    );
    let detail = text_of(
        &a,
        "SELECT detail_message FROM sync_conflicts WHERE entity_type = 'inventory_transactions'
           AND conflict_type = 'COUNT_APPROVAL_RECORD_INVALID'",
        vec![],
    )
    .await;
    assert!(
        detail.contains("which no count line here names") && detail.contains("undoes no adjustment"),
        "{detail}"
    );
}

/// Review of 07d895a9 (N2): "take theirs" on a count line whose held adjustment is now its own
/// applies that adjustment first, then the line; when the line cannot be written (here it names a
/// warehouse not received), nothing is kept: the adjustment is rolled back with it, the stock does
/// not move and the conflict stays open.
#[tokio::test]
async fn take_theirs_keeps_nothing_when_the_line_fails_after_its_movement() {
    let ForgedAdjustment {
        b,
        mut server,
        pair_b,
        counter,
        session_sync,
        line_sync,
        adjustment,
        real,
        ..
    } = forged_adjustment("rollbacktakecounter").await;
    sync_applied(&mut server, "B", &b).await;
    assert_forged_shown_and_held(&b, (&session_sync, pair_b)).await;
    let version = server
        .state
        .get(&("inventory_transactions".to_string(), adjustment.clone()))
        .expect("on the server")
        .row_version;
    inbox_put(
        &b,
        "delta-fix",
        "inventory_transactions",
        &adjustment,
        version + 1,
        &real,
        "pending",
    )
    .await;
    crate::sync::materialize::materialize_inbox(&b, 200, true)
        .await
        .expect("tried");
    assert_eq!(count_movements(&b, &session_sync, "COUNT_ADJUST").await, (0, 0));

    // The line version taken names a warehouse this PC never received.
    let conflict = open_conflict_on(&b, &line_sync).await;
    let line_inbox = scalar_i64(
        &b,
        "SELECT linked_inbox_id FROM sync_conflicts WHERE id = ?",
        vec![conflict.0.into()],
    )
    .await;
    let mut line: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
        &text_of(
            &b,
            "SELECT inbound_payload_json FROM sync_conflicts WHERE id = ?",
            vec![conflict.0.into()],
        )
        .await,
    )
    .expect("payload");
    let warehouse = line
        .keys()
        .find(|k| k.starts_with("warehouse") && k.ends_with("sync_id"))
        .expect("the line names its warehouse")
        .clone();
    line.insert(
        warehouse,
        serde_json::json!(crate::sync::identity::new_entity_sync_id()),
    );
    let json = serde_json::to_string(&line).expect("json");
    exec(
        &b,
        "UPDATE sync_conflicts SET inbound_payload_json = ? WHERE id = ?",
        vec![json.clone().into(), conflict.0.into()],
    )
    .await;
    exec(
        &b,
        "UPDATE sync_inbox SET payload_json = ? WHERE id = ?",
        vec![json.into(), line_inbox.into()],
    )
    .await;

    let refused = format!(
        "{:?}",
        take_theirs(&b, second_person(&b, counter).await, conflict)
            .await
            .expect_err("the line cannot be written")
    );
    assert!(refused.contains("GATE_SYNC_SERVER_VERSION_NOT_APPLIED"), "{refused}");
    assert_eq!(
        count_movements(&b, &session_sync, "COUNT_ADJUST").await,
        (0, 0),
        "the adjustment applied first is rolled back"
    );
    assert_eq!(on_hand_milli(&b, pair_b.0, pair_b.1).await, 10_000);
    assert_eq!(
        conflict_kinds(&b).await,
        vec!["inventory_count_lines:COUNT_APPROVAL_RECORD_INVALID".to_string()],
        "the conflict stays open"
    );
    assert_eq!(
        text_of(
            &b,
            "SELECT apply_status FROM sync_inbox WHERE id = ?",
            vec![line_inbox.into()]
        )
        .await,
        "conflict"
    );
    assert_ne!(
        text_of(
            &b,
            "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = ? ORDER BY row_version DESC LIMIT 1",
            vec![adjustment.clone().into()],
        )
        .await,
        "applied"
    );
}

/// Review of 435c9410 (SF1, SF2, SF3): the reversal a received count line names must undo exactly
/// the adjustment it names as posted — a token reversal, or the real reversal of another count of
/// the same article and location, is refused; a reversal named without a posted adjustment is
/// refused, and such a reversal movement waits (never applied alone).
#[tokio::test]
async fn a_received_reversal_must_undo_the_adjustment_its_line_names() {
    use crate::inventory::domain::ReverseInventoryCountSessionInput;
    let HeldCount {
        a,
        b,
        pair_a,
        counter,
        session_code,
        ..
    } = held_count("reversalqtycounter").await;
    exec_unsent(
        &a,
        "UPDATE stock_balances SET on_hand_qty = on_hand_qty + 10 WHERE article_id = ? AND location_id = ?",
        vec![pair_a.0.into(), pair_a.1.into()],
    )
    .await;
    let approver = second_person(&a, counter).await;
    let reverse = |session: crate::inventory::domain::InventoryCountSession| {
        let a = a.clone();
        async move {
            crate::inventory::controls::reverse_count_session(
                &a,
                ReverseInventoryCountSessionInput {
                    session_id: session.id,
                    expected_row_version: session.row_version,
                    reason: "Pas de seconde approbation possible".into(),
                    actor_id: Some(i64::from(approver)),
                },
            )
            .await
            .expect("reversed");
        }
    };
    reverse(session_by_code(&a, &session_code).await).await;
    // Another count of the same article and location, approved by another person, then reversed: a
    // real COUNT_REVERSAL of another count.
    let other = enter_count(&a, counter, pair_a.0, pair_a.1, 8.0).await;
    let other = post_count_approved_by(&a, counter, approver, other).await;
    reverse(session_by_code(&a, &other.session_code).await).await;

    let reversal = movement_sync_id(&a, &session_sync_of(&a, &session_code).await, "COUNT_REVERSAL").await;
    let other_reversal = movement_sync_id(&a, &session_sync_of(&a, &other.session_code).await, "COUNT_REVERSAL").await;
    let line_sync = line_sync_of(&a, &session_code).await;
    let reversed_line = wire_row(&a, "inventory_count_lines", &line_sync).await;
    assert_eq!(
        reversed_line.get("reversed_transaction_sync_id"),
        Some(&serde_json::json!(reversal))
    );
    // On B the adjustment is held (deferred); the movements reach its inbox.
    let real = wire_row(&a, "inventory_transactions", &reversal).await;
    inbox_put(&b, "delta-r", "inventory_transactions", &reversal, 1, &real, "pending").await;
    let mut token = real.clone();
    token.insert("quantity".into(), serde_json::json!(-0.001));
    let token_sync = crate::sync::identity::new_entity_sync_id();
    inbox_put(
        &b,
        "delta-r",
        "inventory_transactions",
        &token_sync,
        1,
        &token,
        "pending",
    )
    .await;
    let other_row = wire_row(&a, "inventory_transactions", &other_reversal).await;
    inbox_put(
        &b,
        "delta-r",
        "inventory_transactions",
        &other_reversal,
        1,
        &other_row,
        "pending",
    )
    .await;

    let naming = |reversal: &str, posted: Option<serde_json::Value>| {
        let mut p = reversed_line.clone();
        p.insert("reversed_transaction_sync_id".into(), serde_json::json!(reversal));
        if let Some(posted) = posted {
            p.insert("posted_transaction_sync_id".into(), posted);
        }
        p
    };
    let invalid = crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_INVALID;
    assert_eq!(
        line_judged_in(&b, "delta-r", &line_sync, &naming(&reversal, None)).await,
        "pass",
        "its real reversal"
    );
    assert_eq!(
        line_judged_in(&b, "delta-r", &line_sync, &naming(&token_sync, None)).await,
        invalid,
        "a token reversal of −0.001"
    );
    assert_eq!(
        line_judged_in(&b, "delta-r", &line_sync, &naming(&other_reversal, None)).await,
        invalid,
        "the reversal of another count"
    );
    let no_posted = naming(&reversal, Some(serde_json::Value::Null));
    assert_eq!(
        line_judged_in(&b, "delta-r", &line_sync, &no_posted).await,
        invalid,
        "a reversal without a posted adjustment"
    );
    // The reversal movement, whose latest line version names no adjustment for it to undo, waits.
    inbox_put(
        &b,
        "delta-r",
        "inventory_count_lines",
        &line_sync,
        99,
        &no_posted,
        "pending",
    )
    .await;
    let verdict = crate::inventory::count_four_eyes::inbound_count_movement_hold(&b, &reversal, &real)
        .await
        .expect("judged");
    assert_eq!(verdict_code(verdict.as_ref()), "wait", "never applied alone");
}

/// Review of 435c9410 (N1): when the version of a count line naming a movement was set aside by a
/// person's decision on its conflict, the movement says it waits for that conflict to be settled,
/// not for the line to arrive.
#[tokio::test]
async fn a_movement_whose_line_version_was_set_aside_waits_for_that_conflict() {
    let HeldCount { a, b, session_code, .. } = held_count("setasidecounter").await;
    let adjustment = movement_sync_id(&a, &session_sync_of(&a, &session_code).await, "COUNT_ADJUST").await;
    let payload = wire_row(&a, "inventory_transactions", &adjustment).await;
    exec(
        &b,
        "UPDATE sync_inbox SET apply_status = 'superseded'
         WHERE entity_type = 'inventory_count_lines' AND apply_status = 'conflict'",
        vec![],
    )
    .await;
    match crate::inventory::count_four_eyes::inbound_count_movement_hold(&b, &adjustment, &payload)
        .await
        .expect("judged")
    {
        Some(crate::settings::site::InboundVerdict::Defer(why)) => {
            assert!(
                why.contains("waits for the conflict of its count line") && why.contains("COUNT_SELF_APPROVAL"),
                "{why}"
            );
            assert!(!why.contains("has not arrived yet"), "{why}");
        }
        other => panic!("the movement waits: {other:?}"),
    }
}

/// Review of 435c9410 (SF3): a held count of two lines raises one notice to the count-approval
/// holders, not one per line.
#[tokio::test]
async fn a_held_count_of_two_lines_raises_one_notice() {
    use crate::inventory::controls;
    use crate::inventory::domain::{
        ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, PostInventoryCountSessionInput,
        UpsertInventoryCountLineInput,
    };
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let mut factory = RowFactory::new();
    let (article, location) = stocked_pair(&a, &mut factory, 10.0).await;
    let second = factory.create(&a, "articles").await.expect("article");
    exec(
        &a,
        "UPDATE articles SET requires_serial_tracking = 0, requires_batch_tracking = 0,
                requires_expiration = 0, is_critical_spare = 0, is_active = 1 WHERE id = ?",
        vec![second.rowid.into()],
    )
    .await;
    adjust(&a, second.rowid, location.rowid, 10.0).await.expect("stock in");
    sync_clean(&mut server, "A", &a).await;
    sync_clean(&mut server, "B", &b).await;
    sites_made_locally(&a, &b, true, false).await;
    let counter = user_with(&a, "twolinecounter", &[crate::rbac::permissions::INV_MANAGE]).await;
    let warehouse_id = scalar_i64(
        &a,
        "SELECT warehouse_id FROM stock_locations WHERE id = ?",
        vec![location.rowid.into()],
    )
    .await;
    let session = controls::create_count_session(
        &a,
        CreateInventoryCountSessionInput {
            warehouse_id,
            location_id: Some(location.rowid),
            actor_id: Some(i64::from(counter)),
        },
    )
    .await
    .expect("session");
    let session = controls::transition_count_session(&a, move_to(&session, "counting", counter))
        .await
        .expect("counting");
    let mut lines = Vec::new();
    for article_id in [article.rowid, second.rowid] {
        lines.push(
            controls::upsert_count_line(
                &a,
                UpsertInventoryCountLineInput {
                    session_id: session.id,
                    article_id,
                    location_id: location.rowid,
                    counted_qty: 7.0,
                    variance_reason_code: Some("COUNT_ERROR".into()),
                    expected_row_version: None,
                    actor_id: Some(i64::from(counter)),
                },
            )
            .await
            .expect("line"),
        );
    }
    let session = controls::transition_count_session(&a, move_to(&session, "submitted", counter))
        .await
        .expect("submitted");
    for line in &lines {
        let approved = controls::approve_count_line(
            &a,
            ApproveInventoryCountLineInput {
                line_id: line.id,
                expected_row_version: line.row_version,
                reviewer_id: i64::from(counter),
                reviewer_evidence: "Bac vérifié au comptage".into(),
            },
        )
        .await
        .expect("self-approved under A's opt-out");
        assert_eq!(approved.self_approved, 1);
    }
    let session = controls::transition_count_session(&a, move_to(&session, "approved", counter))
        .await
        .expect("approved");
    controls::post_count_session(
        &a,
        PostInventoryCountSessionInput {
            session_id: session.id,
            expected_row_version: session.row_version,
            actor_id: Some(i64::from(counter)),
        },
    )
    .await
    .expect("posted");
    sync_applied(&mut server, "A", &a).await;
    sync_applied(&mut server, "B", &b).await;
    let kinds = conflict_kinds(&b).await;
    assert_eq!(
        kinds
            .iter()
            .filter(|k| k.as_str() == "inventory_count_lines:COUNT_SELF_APPROVAL")
            .count(),
        2,
        "both lines held: {kinds:?}"
    );
    assert_eq!(
        notice_events(&b, crate::inventory::count_four_eyes::SECOND_APPROVAL_CATEGORY).await,
        1,
        "one notice per held count, not per line"
    );
}
