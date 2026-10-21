//! Stock to reconcile after sync: "négatif — à rapprocher" and "double comptage — à vérifier"
//! (CMMS Advisor 2026-10-20, review ruling S1).
//!
//! A stock movement received from another PC is applied as received: it is a physical fact and the
//! ledger must be the same on every PC. Two PCs that each issued the last units of a location
//! therefore leave it below zero on both. Such a balance is not refused and not a sync conflict:
//! - on-hand stock below zero: one review item per article and location (`stock_negative_reviews`,
//!   at most one open per pair), whatever movement brought it there;
//! - on-hand stock at zero or more but more reserved than held: a warning only ("réservations
//!   supérieures au stock"), derived from the balance, never stored and never a review item;
//! - two counts of the same article and location posted on different PCs, each while the location
//!   stood below zero on its PC, neither knowing the other, whatever path recorded them (this list,
//!   a count session):
//!   each corrected the stock by its own variance, so it is corrected twice. A "double comptage — à
//!   vérifier" item, derived from the synchronised count lines (CMMS Advisor, S1 follow-up).
//!
//! Determinism. A negative item is open exactly while the location's on-hand stock is below zero:
//! [`reconcile_pair_in`] opens or closes it from the balance, after each received movement
//! (`sync::apply_hooks`), after the count that resolves it, and for every balance before the list is
//! read (local movements made by the inventory commands are caught there). Balances are derived from
//! the synchronised ledger, so every PC holding the same movements shows the same open items,
//! whatever order they arrived in. The items are this PC's projection and are not synchronised
//! (`sync::registry`, derived projection); what resolves them — the count session, its movement and
//! its audit — is. A double count is computed from the count lines alone: every count posted here
//! records the posted counts of its pair that this PC knew and that no other known count had
//! followed (`count_follows_json`, with this PC's code and the count's path); the counts no other
//! count follows are the heads, and two heads or more from different PCs, each counted while the
//! location stood below zero on its PC (the stock on record kept on the line), are a double count.
//! A later count
//! recorded with both known follows them both, which closes it on every PC that has received it.
//!
//! Resolution. A stock count of that article at that location ([`resolve_by_count`], `inv.manage`,
//! plant-wide while stores have no site), with the variance reason and a reason of at least
//! [`REVIEW_REASON_MIN_CHARS`] characters (the count-approval rule): it is posted through the count
//! workflow (`controls::record_location_count_in`) and audited. It resolves a negative item and a
//! double count alike. A later entry that brings the stock back to zero or more also closes a
//! negative item; how is kept: a goods receipt or a transfer in is "régularisé par réception", a
//! return naming its source document (the work-order issue it returns, the repair order) is
//! "régularisé par retour", a manual entry or a return without source is an adjustment (its reason
//! is required where it is made: `queries::adjust_stock_full`). A receipt into a location below
//! zero is accepted and the item stays open until the stock is zero or more
//! ([`balance_change_allowed`]). There is no reject and no "accept anyway".
//!
//! Notice. The inv.manage holders are told once per item, in-app (watcher [`NOTIFICATION_CATEGORY`],
//! `notifications::watchers`), with the same identity on every PC.

use std::collections::{BTreeSet, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait, Value};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Shortest reason accepted for the count that resolves an item (characters, trimmed): the rule of
/// the count-line approval (CMMS Advisor decision 3).
pub const REVIEW_REASON_MIN_CHARS: usize = 8;

/// Quantities closer to zero than this are zero (balances are sums of decimal movements).
pub(crate) const QTY_TOLERANCE: f64 = 1e-9;

/// Notification category (and watcher) of the stock to reconcile (migration 251).
pub const NOTIFICATION_CATEGORY: &str = "stock_to_reconcile";

/// Where the stock to reconcile is listed (Inventory › Controls; `listLinks.inventory` on screen).
pub const LIST_URL: &str = "/inventory?stock=to_reconcile";

/// Closed by a stock count (or the reversal of one).
pub const CLOSURE_COUNT: &str = "count";
/// Closed by a manual stock entry (counted as a count resolution, CMMS Advisor decision 2).
pub const CLOSURE_ADJUSTMENT: &str = "adjustment";
/// Closed by a goods receipt or a transfer in: "régularisé par réception".
pub const CLOSURE_RECEIPT: &str = "receipt";
/// Closed by stock returned to the store naming its source (the work-order issue, the repair order):
/// "régularisé par retour".
pub const CLOSURE_RETURN: &str = "return";
/// Back to zero or more without a stock entry on record since the item opened (a data repair).
pub const CLOSURE_OTHER: &str = "other";

/// `(movement types, closure kind)` of the stock entries that close an item.
const CLOSING_MOVEMENTS: &[(&[&str], &str)] = &[
    (&["COUNT_ADJUST", "COUNT_REVERSAL"], CLOSURE_COUNT),
    (&["ADJUST_IN"], CLOSURE_ADJUSTMENT),
    (&["GR_ACCEPT", "TRANSFER_IN"], CLOSURE_RECEIPT),
];

pub const REFUSED_REASON_SHORT: &str = "NEGATIVE_STOCK_REASON_SHORT";
pub const REFUSED_COUNT_INVALID: &str = "NEGATIVE_STOCK_COUNT_INVALID";
pub const REFUSED_VARIANCE_REASON_REQUIRED: &str = "NEGATIVE_STOCK_VARIANCE_REASON_REQUIRED";
pub const REFUSED_NOT_OPEN: &str = "NEGATIVE_STOCK_NOT_OPEN";
pub const REFUSED_ENTRY_REASON_SHORT: &str = "NEGATIVE_STOCK_ENTRY_REASON_SHORT";

/// A balance to reconcile (open) or reconciled (closed), with the location's balance now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NegativeStockReview {
    pub id: i64,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub warehouse_code: String,
    /// `open` or `closed`.
    pub status: String,
    /// When this PC found the balance below zero.
    pub opened_at: String,
    /// The movement that took the stock below zero, when known.
    pub opening_transaction_id: Option<i64>,
    pub opening_movement_type: Option<String>,
    pub closed_at: Option<String>,
    /// `count`, `adjustment`, `receipt` ("régularisé par réception"), `return` or `other`.
    pub closure_kind: Option<String>,
    pub closing_transaction_id: Option<i64>,
    pub closing_movement_type: Option<String>,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
}

/// A location holding stock (zero or more) with more reserved than held: a warning, not an item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverReservationWarning {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub warehouse_code: String,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
}

/// One posted count: who posted it, on which PC, by which path, when, and the quantity counted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReconciliationCount {
    /// Sync id of the count line.
    pub count_sync_id: String,
    pub session_code: String,
    /// [`COUNT_PATH_RECONCILIATION`] or [`COUNT_PATH_COUNT_SESSION`].
    pub path: String,
    pub counted_by: Option<String>,
    pub device_code: String,
    pub counted_at: String,
    /// The stock on record the count was set against, on the PC that posted it.
    pub system_qty: f64,
    pub counted_qty: f64,
    pub variance_qty: f64,
}

/// "Double comptage — à vérifier": counts of one article and location posted on different PCs since
/// it last went below zero, none knowing the others.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoubleCountReview {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub warehouse_code: String,
    /// Oldest first.
    pub counts: Vec<ReconciliationCount>,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NegativeStockReviewList {
    /// Open items first, then the closed ones when asked for.
    pub items: Vec<NegativeStockReview>,
    pub double_counts: Vec<DoubleCountReview>,
    pub over_reservations: Vec<OverReservationWarning>,
}

/// Counts for the sync centre, derived from the balances and the count sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegativeStockSummary {
    pub negative_count: i64,
    pub double_count_count: i64,
    pub over_reserved_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveNegativeStockInput {
    pub article_id: i64,
    pub location_id: i64,
    /// Quantity physically counted at the location (zero or more).
    pub counted_qty: f64,
    /// Code of the plant's variance-reason list (Données de référence, `inventory.variance_reason`).
    pub variance_reason_code: String,
    pub reason: String,
}

/// What a resolution recorded, and the negative item it closed (none when only a double count was
/// to verify).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NegativeStockResolution {
    pub session_code: String,
    pub counted_qty: f64,
    pub variance_qty: f64,
    pub review: Option<NegativeStockReview>,
}

fn refused(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn sqlite(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

/// How a stock entry closes an item. A return is a documented inflow only when it names its source
/// document (`documented`: the reservation of the work-order issue, the repair order); without it,
/// it is an adjustment (CMMS Advisor).
fn closure_kind_of(movement_type: &str, documented: bool) -> &'static str {
    if matches!(movement_type, "RETURN" | "REPAIRABLE_RETURN") {
        return if documented { CLOSURE_RETURN } else { CLOSURE_ADJUSTMENT };
    }
    CLOSING_MOVEMENTS
        .iter()
        .find(|(types, _)| types.contains(&movement_type))
        .map_or(CLOSURE_OTHER, |(_, kind)| kind)
}

/// Whether a stock balance may change from `current` to `next` (`(on hand, reserved)`): never below
/// zero nor more reserved than held, except that a location already there may be improved — a
/// receipt, a transfer in, a return, a released reservation or an entry is accepted as long as
/// neither the on-hand nor the available stock goes down (CMMS Advisor decision 1); its item stays
/// open until the stock is zero or more.
pub(crate) fn balance_change_allowed(current: (f64, f64), next: (f64, f64)) -> bool {
    let (on_hand, reserved) = current;
    let (next_on_hand, next_reserved) = next;
    let next_available = next_on_hand - next_reserved;
    if next_reserved < 0.0 {
        return false;
    }
    if next_on_hand >= 0.0 && next_available >= 0.0 {
        return true;
    }
    next_on_hand >= on_hand - QTY_TOLERANCE && next_available >= (on_hand - reserved) - QTY_TOLERANCE
}

/// A manual entry at a location below zero is a count resolution (CMMS Advisor decision 2): it
/// needs a reason of at least [`REVIEW_REASON_MIN_CHARS`] characters (the movement's comment).
pub(crate) fn check_entry_reason(current_on_hand: f64, delta: f64, reason: Option<&str>) -> AppResult<()> {
    if delta <= 0.0 || current_on_hand >= -QTY_TOLERANCE {
        return Ok(());
    }
    if reason.map(str::trim).unwrap_or_default().chars().count() < REVIEW_REASON_MIN_CHARS {
        return Err(refused(
            REFUSED_ENTRY_REASON_SHORT,
            format!(
                "This location is below zero: an entry here reconciles it and needs a reason (at least {REVIEW_REASON_MIN_CHARS} characters)."
            ),
            &[("min", REVIEW_REASON_MIN_CHARS.to_string())],
        ));
    }
    Ok(())
}

/// Opens or closes the item of one article and location from its balance: open while the on-hand
/// stock is below zero (one open item per pair), closed as soon as it is zero or more, noted with
/// the latest stock entry since it opened.
pub async fn reconcile_pair_in(conn: &impl ConnectionTrait, article_id: i64, location_id: i64) -> AppResult<()> {
    let on_hand = conn
        .query_one(sqlite(
            "SELECT on_hand_qty FROM stock_balances WHERE article_id = ? AND location_id = ?",
            vec![article_id.into(), location_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<f64>("", "on_hand_qty"))
        .transpose()?;
    let negative = on_hand.is_some_and(|q| q < -QTY_TOLERANCE);
    let open = conn
        .query_one(sqlite(
            "SELECT id, opening_transaction_id, opened_at FROM stock_negative_reviews
             WHERE article_id = ? AND location_id = ? AND status = 'open'",
            vec![article_id.into(), location_id.into()],
        ))
        .await?;
    let delta = crate::inventory::ledger::on_hand_delta_sql("t");
    match (negative, open) {
        (true, None) => {
            let opening = conn
                .query_one(sqlite(
                    &format!(
                        "SELECT t.id FROM inventory_transactions t
                         WHERE t.article_id = ? AND t.location_id = ? AND ({delta}) < 0
                         ORDER BY t.id DESC LIMIT 1"
                    ),
                    vec![article_id.into(), location_id.into()],
                ))
                .await?
                .map(|row| row.try_get::<i64>("", "id"))
                .transpose()?;
            // A reconcile running at the same time may have opened it: one open row per pair.
            conn.execute(sqlite(
                "INSERT INTO stock_negative_reviews
                    (article_id, location_id, status, opened_at, opening_transaction_id)
                 VALUES (?, ?, 'open', ?, ?)
                 ON CONFLICT(article_id, location_id) WHERE status = 'open' DO NOTHING",
                vec![
                    article_id.into(),
                    location_id.into(),
                    now_iso().into(),
                    opening.map_or(Value::BigInt(None), Value::from),
                ],
            ))
            .await?;
        }
        (false, Some(item)) => {
            let item_id: i64 = item.try_get("", "id")?;
            let opening: Option<i64> = item.try_get("", "opening_transaction_id")?;
            let opened_at: String = item.try_get("", "opened_at")?;
            let closing = conn
                .query_one(sqlite(
                    &format!(
                        "SELECT t.id, t.movement_type, t.performed_at,
                                (t.reservation_id IS NOT NULL OR t.source_id IS NOT NULL) AS documented
                         FROM inventory_transactions t
                         WHERE t.article_id = ? AND t.location_id = ? AND t.id > ? AND ({delta}) > 0
                         ORDER BY t.id DESC LIMIT 1"
                    ),
                    vec![article_id.into(), location_id.into(), opening.unwrap_or(0).into()],
                ))
                .await?;
            let (kind, closing_id, closed_at) = match closing {
                Some(row) => {
                    let movement_type: String = row.try_get("", "movement_type")?;
                    let performed_at: Option<String> = row.try_get("", "performed_at")?;
                    let documented = row.try_get::<i64>("", "documented")? != 0;
                    // The time of the movement that closed it, never before the item opened here
                    // (a movement made earlier on another PC may arrive after the item opened).
                    let closed_at = performed_at.filter(|at| *at > opened_at).unwrap_or(opened_at);
                    (
                        closure_kind_of(&movement_type, documented),
                        Some(row.try_get::<i64>("", "id")?),
                        closed_at,
                    )
                }
                None => (CLOSURE_OTHER, None, now_iso()),
            };
            conn.execute(sqlite(
                "UPDATE stock_negative_reviews
                 SET status = 'closed', closed_at = ?, closure_kind = ?, closing_transaction_id = ?
                 WHERE id = ? AND status = 'open'",
                vec![
                    closed_at.into(),
                    kind.into(),
                    closing_id.map_or(Value::BigInt(None), Value::from),
                    item_id.into(),
                ],
            ))
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// [`reconcile_pair_in`] for every balance below zero and every open item.
pub async fn reconcile_all_in(conn: &impl ConnectionTrait) -> AppResult<()> {
    let pairs = conn
        .query_all(sqlite(
            "SELECT article_id, location_id FROM stock_balances WHERE on_hand_qty < ?
             UNION
             SELECT article_id, location_id FROM stock_negative_reviews WHERE status = 'open'",
            vec![(-QTY_TOLERANCE).into()],
        ))
        .await?;
    for row in pairs {
        reconcile_pair_in(conn, row.try_get("", "article_id")?, row.try_get("", "location_id")?).await?;
    }
    Ok(())
}

/// Path of a count recorded from the stock-to-reconcile list.
pub const COUNT_PATH_RECONCILIATION: &str = "reconciliation";
/// Path of a count posted through a count session (cycle count, inventory campaign).
pub const COUNT_PATH_COUNT_SESSION: &str = "count_session";

/// The posted counts of an article and location that no other count follows, oldest first (time
/// posted, then line sync id: the same order on every PC). Every path counts (the list, count
/// sessions); a count line posted before this version carries no mark and takes no part. A count
/// whose session was reversed is no longer a head (its correction was undone), but what it follows
/// stays followed.
pub(crate) async fn reconciliation_heads(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<Vec<ReconciliationCount>> {
    let rows = conn
        .query_all(sqlite(
            "SELECT cl.entity_sync_id, cs.session_code, cs.status, cl.count_device_code, cl.count_follows_json,
                    cl.count_path, cs.posted_at, ua.display_name, ua.username, cl.system_qty, cl.counted_qty,
                    cl.variance_qty
             FROM inventory_count_lines cl
             JOIN inventory_count_sessions cs ON cs.id = cl.session_id
             LEFT JOIN user_accounts ua ON ua.id = cs.posted_by_id
             WHERE cl.count_follows_json IS NOT NULL AND cl.entity_sync_id IS NOT NULL
               AND cs.status IN ('posted', 'reversed')
               AND cl.article_id = ? AND cl.location_id = ?
             ORDER BY cs.posted_at, cl.entity_sync_id",
            vec![article_id.into(), location_id.into()],
        ))
        .await?;
    let mut followed: HashSet<String> = HashSet::new();
    let mut counts = Vec::new();
    for row in &rows {
        let follows_json: String = row.try_get("", "count_follows_json")?;
        // A list this version cannot read follows nothing: its counts stay heads (fail-safe).
        if let Ok(follows) = serde_json::from_str::<Vec<String>>(&follows_json) {
            followed.extend(follows);
        }
        let status: String = row.try_get("", "status")?;
        if status == "reversed" {
            continue;
        }
        let display: Option<String> = row.try_get("", "display_name")?;
        let username: Option<String> = row.try_get("", "username")?;
        counts.push(ReconciliationCount {
            count_sync_id: row.try_get("", "entity_sync_id")?,
            session_code: row.try_get("", "session_code")?,
            path: row
                .try_get::<Option<String>>("", "count_path")?
                .unwrap_or_else(|| COUNT_PATH_COUNT_SESSION.to_string()),
            counted_by: display.filter(|d| !d.trim().is_empty()).or(username),
            device_code: row
                .try_get::<Option<String>>("", "count_device_code")?
                .unwrap_or_default(),
            counted_at: row.try_get::<Option<String>>("", "posted_at")?.unwrap_or_default(),
            system_qty: row.try_get("", "system_qty")?,
            counted_qty: row.try_get("", "counted_qty")?,
            variance_qty: row.try_get("", "variance_qty")?,
        });
    }
    counts.retain(|c| !followed.contains(&c.count_sync_id));
    Ok(counts)
}

/// The mark of a count being posted here for an article and location: this PC's code and the JSON
/// list of the counts it follows (every head this PC knows). Written on the count line by every
/// count path (`controls::post_count_session`, `controls::record_location_count_in`).
pub(crate) async fn count_mark(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<(String, String)> {
    let heads = reconciliation_heads(conn, article_id, location_id).await?;
    let follows: Vec<&str> = heads.iter().map(|c| c.count_sync_id.as_str()).collect();
    let follows_json = serde_json::to_string(&follows).map_err(|e| AppError::Internal(e.into()))?;
    Ok((crate::sync::device_code::this_device_code(conn).await?, follows_json))
}

/// The heads counted while the location stood below zero on the PC that posted them (after its
/// item opened there: the stock on record the count was set against, kept on the line), posted on
/// two PCs or more: a double count to verify. Read from the synchronised count lines only, so every
/// PC holding them gives the same answer.
pub(crate) async fn double_count_heads(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<Option<Vec<ReconciliationCount>>> {
    let heads: Vec<ReconciliationCount> = reconciliation_heads(conn, article_id, location_id)
        .await?
        .into_iter()
        .filter(|c| c.system_qty < -QTY_TOLERANCE)
        .collect();
    let devices = heads.iter().map(|c| c.device_code.as_str()).collect::<BTreeSet<_>>();
    Ok((devices.len() >= 2).then_some(heads))
}

/// Every article and location with a double count to verify.
pub(crate) async fn double_count_pairs(
    conn: &impl ConnectionTrait,
) -> AppResult<Vec<(i64, i64, Vec<ReconciliationCount>)>> {
    let pairs = conn
        .query_all(sqlite(
            "SELECT DISTINCT article_id, location_id FROM inventory_count_lines
             WHERE count_follows_json IS NOT NULL
             ORDER BY article_id, location_id",
            vec![],
        ))
        .await?;
    let mut out = Vec::new();
    for row in pairs {
        let (article_id, location_id): (i64, i64) = (row.try_get("", "article_id")?, row.try_get("", "location_id")?);
        if let Some(heads) = double_count_heads(conn, article_id, location_id).await? {
            out.push((article_id, location_id, heads));
        }
    }
    Ok(out)
}

const REVIEW_SELECT: &str = "SELECT r.id, r.article_id, a.article_code, a.article_name, r.location_id,
        sl.code AS location_code, w.code AS warehouse_code, r.status, r.opened_at,
        r.opening_transaction_id, ot.movement_type AS opening_movement_type, r.closed_at, r.closure_kind,
        r.closing_transaction_id, ct.movement_type AS closing_movement_type,
        COALESCE(sb.on_hand_qty, 0) AS on_hand_qty, COALESCE(sb.reserved_qty, 0) AS reserved_qty,
        COALESCE(sb.available_qty, 0) AS available_qty
     FROM stock_negative_reviews r
     JOIN articles a ON a.id = r.article_id
     JOIN stock_locations sl ON sl.id = r.location_id
     JOIN warehouses w ON w.id = sl.warehouse_id
     LEFT JOIN stock_balances sb ON sb.article_id = r.article_id AND sb.location_id = r.location_id
     LEFT JOIN inventory_transactions ot ON ot.id = r.opening_transaction_id
     LEFT JOIN inventory_transactions ct ON ct.id = r.closing_transaction_id";

fn map_review(row: &QueryResult) -> AppResult<NegativeStockReview> {
    Ok(NegativeStockReview {
        id: row.try_get("", "id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        warehouse_code: row.try_get("", "warehouse_code")?,
        status: row.try_get("", "status")?,
        opened_at: row.try_get("", "opened_at")?,
        opening_transaction_id: row.try_get("", "opening_transaction_id")?,
        opening_movement_type: row.try_get("", "opening_movement_type")?,
        closed_at: row.try_get("", "closed_at")?,
        closure_kind: row.try_get("", "closure_kind")?,
        closing_transaction_id: row.try_get("", "closing_transaction_id")?,
        closing_movement_type: row.try_get("", "closing_movement_type")?,
        on_hand_qty: row.try_get("", "on_hand_qty")?,
        reserved_qty: row.try_get("", "reserved_qty")?,
        available_qty: row.try_get("", "available_qty")?,
    })
}

async fn load_review(conn: &impl ConnectionTrait, id: i64) -> AppResult<NegativeStockReview> {
    let row = conn
        .query_one(sqlite(&format!("{REVIEW_SELECT} WHERE r.id = ?"), vec![id.into()]))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "stock_negative_reviews".to_string(),
            id: id.to_string(),
        })?;
    map_review(&row)
}

/// Article, location and balance columns of one pair.
const PAIR_SELECT: &str = "SELECT a.id AS article_id, a.article_code, a.article_name, sl.id AS location_id,
        sl.code AS location_code, w.code AS warehouse_code,
        COALESCE(sb.on_hand_qty, 0) AS on_hand_qty, COALESCE(sb.reserved_qty, 0) AS reserved_qty,
        COALESCE(sb.available_qty, 0) AS available_qty
     FROM articles a
     JOIN stock_locations sl ON sl.id = ?
     JOIN warehouses w ON w.id = sl.warehouse_id
     LEFT JOIN stock_balances sb ON sb.article_id = a.id AND sb.location_id = sl.id
     WHERE a.id = ?";

/// The items to reconcile (and the closed ones when `include_closed`), the double counts to verify
/// and the over-reservation warnings. The items are first brought in line with the balances.
pub async fn list_reviews(db: &DatabaseConnection, include_closed: bool) -> AppResult<NegativeStockReviewList> {
    let tx = db.begin().await?;
    reconcile_all_in(&tx).await?;
    tx.commit().await?;
    let filter = if include_closed { "" } else { " WHERE r.status = 'open'" };
    let items = db
        .query_all(sqlite(
            &format!(
                "{REVIEW_SELECT}{filter}
                 ORDER BY CASE r.status WHEN 'open' THEN 0 ELSE 1 END,
                          COALESCE(r.closed_at, r.opened_at) DESC, r.id DESC"
            ),
            vec![],
        ))
        .await?
        .iter()
        .map(map_review)
        .collect::<AppResult<Vec<_>>>()?;
    let mut double_counts = Vec::new();
    for (article_id, location_id, counts) in double_count_pairs(db).await? {
        let Some(row) = db
            .query_one(sqlite(PAIR_SELECT, vec![location_id.into(), article_id.into()]))
            .await?
        else {
            continue;
        };
        double_counts.push(DoubleCountReview {
            article_id,
            article_code: row.try_get("", "article_code")?,
            article_name: row.try_get("", "article_name")?,
            location_id,
            location_code: row.try_get("", "location_code")?,
            warehouse_code: row.try_get("", "warehouse_code")?,
            counts,
            on_hand_qty: row.try_get("", "on_hand_qty")?,
            reserved_qty: row.try_get("", "reserved_qty")?,
            available_qty: row.try_get("", "available_qty")?,
        });
    }
    let over_reservations = db
        .query_all(sqlite(
            "SELECT sb.article_id, a.article_code, a.article_name, sb.location_id, sl.code AS location_code,
                    w.code AS warehouse_code, sb.on_hand_qty, sb.reserved_qty, sb.available_qty
             FROM stock_balances sb
             JOIN articles a ON a.id = sb.article_id
             JOIN stock_locations sl ON sl.id = sb.location_id
             JOIN warehouses w ON w.id = sl.warehouse_id
             WHERE sb.on_hand_qty >= ? AND sb.available_qty < ?
             ORDER BY a.article_code, sl.code",
            vec![(-QTY_TOLERANCE).into(), (-QTY_TOLERANCE).into()],
        ))
        .await?
        .iter()
        .map(|row| -> AppResult<OverReservationWarning> {
            Ok(OverReservationWarning {
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(NegativeStockReviewList {
        items,
        double_counts,
        over_reservations,
    })
}

/// How many locations are below zero, have a double count to verify, and hold more reserved than
/// on hand (sync centre).
pub async fn summary(db: &DatabaseConnection) -> AppResult<NegativeStockSummary> {
    let row = db
        .query_one(sqlite(
            "SELECT
                COALESCE(SUM(CASE WHEN on_hand_qty < ? THEN 1 ELSE 0 END), 0) AS negative_count,
                COALESCE(SUM(CASE WHEN on_hand_qty >= ? AND available_qty < ? THEN 1 ELSE 0 END), 0)
                    AS over_reserved_count
             FROM stock_balances",
            vec![
                (-QTY_TOLERANCE).into(),
                (-QTY_TOLERANCE).into(),
                (-QTY_TOLERANCE).into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("stock balance summary returned no row".to_string()))?;
    Ok(NegativeStockSummary {
        negative_count: row.try_get("", "negative_count")?,
        double_count_count: i64::try_from(double_count_pairs(db).await?.len()).unwrap_or(i64::MAX),
        over_reserved_count: row.try_get("", "over_reserved_count")?,
    })
}

/// The most recent movement that took the on-hand stock of an article at a location from zero or
/// more to below zero, in the order every PC shares (time, then sync id), walked back from today's
/// level; `None` when the ledger shows it below zero from the start. Names the occurrence of the
/// notice the same way on every PC.
pub(crate) async fn negative_crossing(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
    on_hand_now: f64,
) -> AppResult<Option<(String, Option<String>)>> {
    let rows = conn
        .query_all(sqlite(
            "SELECT entity_sync_id, movement_type, quantity, performed_at FROM inventory_transactions
             WHERE article_id = ? AND location_id = ? AND entity_sync_id IS NOT NULL
             ORDER BY performed_at DESC, entity_sync_id DESC",
            vec![article_id.into(), location_id.into()],
        ))
        .await?;
    let mut on_hand = on_hand_now;
    for row in &rows {
        let movement: String = row.try_get("", "movement_type")?;
        let quantity: f64 = row.try_get("", "quantity")?;
        let Some((delta, _)) = crate::inventory::ledger::movement_balance_effect(&movement, quantity) else {
            continue;
        };
        let before = on_hand - delta;
        if before >= -QTY_TOLERANCE && on_hand < -QTY_TOLERANCE {
            return Ok(Some((
                row.try_get("", "entity_sync_id")?,
                row.try_get("", "performed_at")?,
            )));
        }
        on_hand = before;
    }
    Ok(None)
}

/// Resolves the stock to reconcile of an article and location — the negative item, a double count
/// to verify, or both — by a stock count recorded there by `user_id`: needs `inv.manage`
/// (plant-wide: stores have no site yet, as for stock adjustments), a reason of at least
/// [`REVIEW_REASON_MIN_CHARS`] characters, the quantity counted (zero or more) and a code of the
/// variance-reason list. The count is posted through the count workflow, marked as following every
/// reconciliation count of the pair this PC knows, and audited; the negative item closes since the
/// stock is now the counted quantity.
pub async fn resolve_by_count(
    db: &DatabaseConnection,
    user_id: i32,
    input: ResolveNegativeStockInput,
) -> AppResult<NegativeStockResolution> {
    let allowed = crate::auth::rbac::check_permission(
        db,
        user_id,
        crate::rbac::permissions::INV_MANAGE,
        &crate::auth::rbac::PermissionScope::Global,
    )
    .await?;
    if !allowed {
        return Err(AppError::PermissionDenied(format!(
            "Permission requise : {}",
            crate::rbac::permissions::INV_MANAGE
        )));
    }
    let reason = input.reason.trim();
    if reason.chars().count() < REVIEW_REASON_MIN_CHARS {
        return Err(refused(
            REFUSED_REASON_SHORT,
            format!("Give the reason of the count (at least {REVIEW_REASON_MIN_CHARS} characters)."),
            &[("min", REVIEW_REASON_MIN_CHARS.to_string())],
        ));
    }
    if !input.counted_qty.is_finite() || input.counted_qty < 0.0 {
        return Err(refused(
            REFUSED_COUNT_INVALID,
            "The counted quantity must be zero or more.",
            &[],
        ));
    }
    let variance_reason_code = input.variance_reason_code.trim();
    if variance_reason_code.is_empty() {
        return Err(refused(
            REFUSED_VARIANCE_REASON_REQUIRED,
            "Choose the variance reason of the count.",
            &[],
        ));
    }

    let tx = db.begin().await?;
    reconcile_pair_in(&tx, input.article_id, input.location_id).await?;
    let item_id = tx
        .query_one(sqlite(
            "SELECT id FROM stock_negative_reviews WHERE article_id = ? AND location_id = ? AND status = 'open'",
            vec![input.article_id.into(), input.location_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "id"))
        .transpose()?;
    let double_count = double_count_heads(&tx, input.article_id, input.location_id)
        .await?
        .is_some();
    if item_id.is_none() && !double_count {
        return Err(refused(
            REFUSED_NOT_OPEN,
            "This location's stock is no longer to reconcile.",
            &[],
        ));
    }
    let (device_code, follows_json) = count_mark(&tx, input.article_id, input.location_id).await?;
    let actor_id = i64::from(user_id);
    let posted = crate::inventory::controls::record_location_count_in(
        &tx,
        &crate::inventory::controls::LocationCount {
            article_id: input.article_id,
            location_id: input.location_id,
            counted_qty: input.counted_qty,
            variance_reason_code,
            evidence: reason,
            actor_id,
            mark: (&device_code, &follows_json),
        },
    )
    .await?;
    reconcile_pair_in(&tx, input.article_id, input.location_id).await?;
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "inventory.negative_stock.counted".to_string(),
            target_type: Some("stock_location_article".to_string()),
            target_id: Some(format!("{}:{}", input.article_id, input.location_id)),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Stock to reconcile resolved by a count",
                "article_id": input.article_id,
                "location_id": input.location_id,
                "negative_review_id": item_id,
                "double_count": double_count,
                "follows_counts": serde_json::from_str::<serde_json::Value>(&follows_json).unwrap_or_default(),
                "device_code": device_code,
                "system_qty": posted.system_qty,
                "counted_qty": input.counted_qty,
                "variance_qty": posted.variance_qty,
                "variance_reason_code": variance_reason_code,
                "reason": reason,
                "count_session_id": posted.session_id,
                "count_session_code": posted.session_code,
                "count_line_id": posted.line_id,
                "transaction_id": posted.transaction_id,
            })),
        },
    )
    .await?;
    tx.commit().await?;
    let review = match item_id {
        Some(id) => Some(load_review(db, id).await?),
        None => None,
    };
    Ok(NegativeStockResolution {
        session_code: posted.session_code,
        counted_qty: input.counted_qty,
        variance_qty: posted.variance_qty,
        review,
    })
}
