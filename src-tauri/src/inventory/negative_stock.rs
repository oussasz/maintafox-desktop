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
//! - two counts of the same article and location made on different PCs, each while the location
//!   stood below zero on its PC, neither knowing the other even indirectly, whatever path recorded
//!   them (this list, a count session, an inventory campaign): each corrected the stock by its own
//!   variance, so it is corrected twice. A "double comptage — à vérifier" item, derived from the
//!   synchronised count lines (CMMS Advisor, S1 follow-up; rule in [`double_count_of`]).
//! - one count of a session, article and location whose movement is in the ledger twice (the same
//!   session posted, or reversed, on two PCs by versions that gave each posting its own id; AU-100):
//!   a "comptage passé deux fois — à vérifier" item, derived from the synchronised ledger (CMMS
//!   Advisor decision (a), [`DoublePostedCountReview`]), closed only by a new count of that article
//!   and location made on a PC holding every one of its movements ([`resolve_by_count`]).
//!
//! Determinism. A negative item is open exactly while the location's on-hand stock is below zero:
//! [`reconcile_pair_in`] opens or closes it from the balance, after each received movement
//! (`sync::apply_hooks`), after the count that resolves it, and for every balance before the list is
//! read (local movements made by the inventory commands are caught there). Balances are derived from
//! the synchronised ledger, so every PC holding the same movements shows the same open items,
//! whatever order they arrived in. The items are this PC's projection and are not synchronised
//! (`sync::registry`, derived projection); what resolves them — the count session, its movement and
//! its audit — is. A double count is computed from the count lines alone: every count records,
//! when its counted quantity and variance are fixed, the posted counts of its pair that this PC
//! knew and that no other known count had followed (`count_follows_json`, with this PC's sync
//! replica id and the count's path). Knowing is transitive through those lists. Two counts made
//! while the location stood below zero on their PC (the stock on record kept on the line), on
//! different PCs, neither knowing the other, are a double count until a later count knows both,
//! which closes it on every PC that has received it, or until one of the two is reversed (the
//! stock is then corrected once): it is closed "par annulation du comptage <ref> (<qui>, <quand>)"
//! and kept in the closed history; the location's negative item, if the stock is still below zero,
//! stays open on its own (CMMS Advisor ruling, intended rule). A count only follows counts whose
//! movement is already in this PC's stock, and on one PC a session whose location was counted and
//! posted since it was opened is refused at posting (`controls::post_count_session`).
//!
//! Resolution. A stock count of that article at that location ([`resolve_by_count`], `inv.manage`,
//! plant-wide while stores have no site), with the variance reason and a reason of at least
//! [`REVIEW_REASON_MIN_CHARS`] characters (the count-approval rule): it is posted through the count
//! workflow (`controls::record_location_count_in`) and audited. It resolves a negative item and a
//! double count alike. A later entry that brings the stock back to zero or more also closes a
//! negative item; how is kept ([`closure_kind_of`], one kind for each stock entry of the ledger): a
//! goods receipt or a transfer in is "régularisé par réception", a return naming its source
//! document (the work-order issue it returns, the repair order — a repair return or a cancelled
//! repair release) is "régularisé par retour", a manual entry made where the location stood below
//! zero (its reason is required there: `queries::adjust_stock_full`, marked
//! [`SOURCE_NEGATIVE_STOCK_ENTRY`]), a count reversal (its reason is required) or a return without
//! source is an adjustment, and a manual entry made on a PC that still saw the location at zero or
//! more — no reason was asked — is "régularisé par saisie antérieure". A receipt into a location
//! below zero is accepted and the item stays open until the stock is zero or more
//! ([`balance_change_allowed`]). There is no reject and no "accept anyway".
//!
//! Notice. The inv.manage holders are told once per item, in-app (watcher [`NOTIFICATION_CATEGORY`],
//! `notifications::watchers`), with the same identity on every PC: the occurrence is the article
//! and location, so a movement arriving late never changes it while the item is open; a later
//! item of the same pair reopens it.

use std::collections::{HashMap, HashSet};

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

/// Closed by a stock count.
pub const CLOSURE_COUNT: &str = "count";
/// Closed by an entry recorded with its reason (at least [`REVIEW_REASON_MIN_CHARS`] characters):
/// a manual entry made where the location stood below zero (CMMS Advisor decision 2), the reversal
/// of a count, or a return that names no source document.
pub const CLOSURE_ADJUSTMENT: &str = "adjustment";
/// Closed by a goods receipt or a transfer in: "régularisé par réception".
pub const CLOSURE_RECEIPT: &str = "receipt";
/// Closed by stock returned to the store naming its source (the work-order issue, the repair order):
/// "régularisé par retour".
pub const CLOSURE_RETURN: &str = "return";
/// Closed by a manual entry made on a PC that still saw the location at zero or more, so no reason
/// was asked: "régularisé par saisie antérieure".
pub const CLOSURE_PRIOR_ENTRY: &str = "prior_entry";
/// Back to zero or more without a stock entry on record since the item opened (a data repair).
pub const CLOSURE_OTHER: &str = "other";

/// `source_type` of a manual entry made where the location stood below zero: its reason was asked
/// and it was audited (`queries::adjust_stock_full`). It travels with the movement, so every PC
/// tells such an entry from one made before the shortage was visible.
pub const SOURCE_NEGATIVE_STOCK_ENTRY: &str = "NEGATIVE_STOCK_ENTRY";

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

/// The reversal of a count session that ended a double count (CMMS Advisor ruling on SF2): "clos par
/// annulation du comptage <ref> (<qui>, <quand>)".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CountReversal {
    /// Sync id of the reversed count line.
    pub count_sync_id: String,
    pub session_code: String,
    pub reversed_by: Option<String>,
    pub reversed_at: String,
}

/// "Double comptage — à vérifier": counts of one article and location posted on different PCs since
/// it last went below zero, none knowing the others. Closed (`closed_by_reversal`) when one of them
/// was reversed: the stock is then corrected once.
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
    /// `None` while it is to verify; the reversal that closed it otherwise.
    pub closed_by_reversal: Option<CountReversal>,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
}

/// One movement of a count posted twice, or one version of a divergent count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoublePostedMovement {
    pub movement_sync_id: String,
    /// Who posted it (the movement's author), when the account is known here.
    pub posted_by: Option<String>,
    pub posted_at: String,
    pub quantity: f64,
    /// The PC that posted it, when known: the stock ledger does not record it; the audit of a
    /// divergent count does, for the version that was replaced.
    pub device_code: Option<String>,
}

/// A count posted twice by older versions (two movements in the ledger).
pub const DOUBLE_POSTING_POSTED_TWICE: &str = "posted_twice";
/// A count posted on two PCs with different quantities: the first pushed is kept everywhere
/// (`inventory::count_sync`), the other is in its audit.
pub const DOUBLE_POSTING_DIVERGENT: &str = "divergent";

/// "Comptage passé deux fois — à vérifier" (CMMS Advisor decision (a), AU-100): one count of a
/// session, article and location whose movement is in the ledger more than once (`COUNT_ADJUST`,
/// or `COUNT_REVERSAL`), from versions that gave each PC's posting its own id. The stock was
/// corrected once per movement. Found from the synchronised ledger alone (the same on every PC that
/// holds the same movements); closed only by a later count of that article and location made on a
/// PC that held every one of those movements ([`count_mark`] names them, [`resolve_by_count`]).
///
/// "Comptage divergent — à vérifier" (kind [`DOUBLE_POSTING_DIVERGENT`]): the same count posted on
/// two PCs whose lines differed in quantity (`inventory::count_sync`): one movement is kept on every
/// PC and the stock follows it, the other version is in the audit event `inventory.count.posted_twice`
/// (`quantity_differs`). Listed from that event — on the PC whose version was replaced, then on every
/// PC it reaches — and closed the same way, by a new count made where that movement was held. Audit
/// rows are accepted from every PC: an edited or forged one can only add such an item (closed by a
/// recount), never move stock; one this version cannot read is left out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoublePostedCountReview {
    /// [`DOUBLE_POSTING_POSTED_TWICE`] or [`DOUBLE_POSTING_DIVERGENT`].
    pub kind: String,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub warehouse_code: String,
    pub session_code: String,
    /// `COUNT_ADJUST` (posted twice) or `COUNT_REVERSAL` (reversed twice).
    pub movement_type: String,
    /// Posted twice: the movements, oldest first (time posted, then sync id). Divergent: the version
    /// kept first, then the versions replaced.
    pub movements: Vec<DoublePostedMovement>,
    /// The count's correction (one movement; divergent: the version kept).
    pub variance_qty: f64,
    /// Posted twice: what the stock moved beyond that correction (the other movements together).
    /// Divergent: how far the replaced version was from the kept one (replaced minus kept).
    pub excess_qty: f64,
    /// The count that closed it (listed with the closed items).
    pub closed_by: Option<ReconciliationCount>,
    pub on_hand_qty: f64,
    pub reserved_qty: f64,
    pub available_qty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NegativeStockReviewList {
    /// Open items first, then the closed ones when asked for.
    pub items: Vec<NegativeStockReview>,
    /// The double counts to verify.
    pub double_counts: Vec<DoubleCountReview>,
    /// The double counts closed by the reversal of one of their counts (when the closed items are
    /// asked for), latest reversal first.
    pub closed_double_counts: Vec<DoubleCountReview>,
    /// The counts posted twice to verify, then (when the closed items are asked for) those a new
    /// count closed.
    pub double_postings: Vec<DoublePostedCountReview>,
    pub closed_double_postings: Vec<DoublePostedCountReview>,
    pub over_reservations: Vec<OverReservationWarning>,
}

/// Counts for the sync centre, derived from the balances and the count sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegativeStockSummary {
    pub negative_count: i64,
    pub double_count_count: i64,
    /// Counts posted twice still to verify ([`DoublePostedCountReview`]).
    pub double_posting_count: i64,
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

/// How a stock entry closes an item, for every movement that can raise the on-hand stock
/// (`ledger::ON_HAND_MOVEMENTS`; a test keeps the two in step). A return is a documented inflow
/// only when it names its source document (`documented`: the reservation of the work-order issue,
/// the repair order); without it, it is an adjustment (CMMS Advisor). A manual entry is an
/// adjustment when it was made where the location stood below zero (`source_type`
/// [`SOURCE_NEGATIVE_STOCK_ENTRY`], its reason asked), otherwise an entry made before the shortage
/// was visible. `None`: the movement never raises the on-hand stock.
pub(crate) fn closure_kind_of(
    movement_type: &str,
    documented: bool,
    source_type: Option<&str>,
) -> Option<&'static str> {
    Some(match movement_type {
        "COUNT_ADJUST" => CLOSURE_COUNT,
        // Undoing a count is not a count: it needs its reason and is kept on the session.
        "COUNT_REVERSAL" => CLOSURE_ADJUSTMENT,
        "ADJUST_IN" if source_type == Some(SOURCE_NEGATIVE_STOCK_ENTRY) => CLOSURE_ADJUSTMENT,
        "ADJUST_IN" => CLOSURE_PRIOR_ENTRY,
        "GR_ACCEPT" | "TRANSFER_IN" => CLOSURE_RECEIPT,
        "RETURN" | "REPAIRABLE_RETURN" | "REPAIRABLE_CANCEL_RESTORE" if documented => CLOSURE_RETURN,
        "RETURN" | "REPAIRABLE_RETURN" | "REPAIRABLE_CANCEL_RESTORE" => CLOSURE_ADJUSTMENT,
        _ => return None,
    })
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
                        "SELECT t.id, t.movement_type, t.performed_at, t.source_type,
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
                    let source_type: Option<String> = row.try_get("", "source_type")?;
                    // The time of the movement that closed it, never before the item opened here
                    // (a movement made earlier on another PC may arrive after the item opened).
                    let closed_at = performed_at.filter(|at| *at > opened_at).unwrap_or(opened_at);
                    (
                        // Only on-hand entries are read here, and each has its kind.
                        closure_kind_of(&movement_type, documented, source_type.as_deref()).unwrap_or(CLOSURE_OTHER),
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

/// One marked count of a pair and what its mark says.
struct CountNode {
    count: ReconciliationCount,
    /// Sync replica id of the PC that recorded it (empty when the line carries none).
    replica_id: String,
    /// The counts it follows directly (the counts this PC knew when its variance was fixed), and the
    /// movements of the counts posted twice that this PC held then ([`count_mark`]).
    follows: Vec<String>,
    /// Its session was reversed: its correction is undone, what it knew stays known.
    reversed: bool,
    /// Who reversed it and when (its session), when reversed.
    reversed_by: Option<String>,
    reversed_at: String,
    /// Its correction is in this PC's stock: no variance, or its movement (and, once reversed, the
    /// reversal's) has arrived here. Only such counts are followed by a new count's mark.
    applied: bool,
}

/// The marked counts of an article and location whose session is posted (or was posted, then
/// reversed), oldest first (time posted, then line sync id: the same order on every PC). Every
/// path counts (the list, count sessions, inventory campaigns); a count line recorded before this
/// version carries no mark and takes no part.
async fn count_nodes(conn: &impl ConnectionTrait, article_id: i64, location_id: i64) -> AppResult<Vec<CountNode>> {
    let rows = conn
        .query_all(sqlite(
            "SELECT cl.entity_sync_id, cs.session_code, cs.status, cl.count_replica_id, cl.count_follows_json,
                    cl.count_path, cs.posted_at, ua.display_name, ua.username, cl.system_qty, cl.counted_qty,
                    cl.variance_qty, cs.reversed_at, ra.display_name AS reversed_display, ra.username AS reversed_username,
                    (ABS(cl.variance_qty) <= 1e-9
                     OR (EXISTS (SELECT 1 FROM inventory_transactions t WHERE t.id = cl.posted_transaction_id)
                         AND (cs.status <> 'reversed'
                              OR EXISTS (SELECT 1 FROM inventory_transactions t
                                         WHERE t.id = cl.reversed_transaction_id)))) AS applied
             FROM inventory_count_lines cl
             JOIN inventory_count_sessions cs ON cs.id = cl.session_id
             LEFT JOIN user_accounts ua ON ua.id = cs.posted_by_id
             LEFT JOIN user_accounts ra ON ra.id = cs.reversed_by_id
             WHERE cl.count_follows_json IS NOT NULL AND cl.entity_sync_id IS NOT NULL
               AND cs.status IN ('posted', 'reversed')
               AND cl.article_id = ? AND cl.location_id = ?
             ORDER BY cs.posted_at, cl.entity_sync_id",
            vec![article_id.into(), location_id.into()],
        ))
        .await?;
    let mut nodes = Vec::with_capacity(rows.len());
    for row in &rows {
        let follows_json: String = row.try_get("", "count_follows_json")?;
        // A list this version cannot read follows nothing: its count stays independent (fail-safe).
        let follows = serde_json::from_str::<Vec<String>>(&follows_json).unwrap_or_default();
        let replica_id = row
            .try_get::<Option<String>>("", "count_replica_id")?
            .unwrap_or_default()
            .trim()
            .to_string();
        let display: Option<String> = row.try_get("", "display_name")?;
        let username: Option<String> = row.try_get("", "username")?;
        let status: String = row.try_get("", "status")?;
        let reversed_display: Option<String> = row.try_get("", "reversed_display")?;
        let reversed_username: Option<String> = row.try_get("", "reversed_username")?;
        nodes.push(CountNode {
            count: ReconciliationCount {
                count_sync_id: row.try_get("", "entity_sync_id")?,
                session_code: row.try_get("", "session_code")?,
                path: row
                    .try_get::<Option<String>>("", "count_path")?
                    .unwrap_or_else(|| COUNT_PATH_COUNT_SESSION.to_string()),
                counted_by: display.filter(|d| !d.trim().is_empty()).or(username),
                device_code: if replica_id.is_empty() {
                    String::new()
                } else {
                    crate::sync::device_code::device_code_for(&replica_id)
                },
                counted_at: row.try_get::<Option<String>>("", "posted_at")?.unwrap_or_default(),
                system_qty: row.try_get("", "system_qty")?,
                counted_qty: row.try_get("", "counted_qty")?,
                variance_qty: row.try_get("", "variance_qty")?,
            },
            replica_id,
            follows,
            reversed: status == "reversed",
            reversed_by: reversed_display.filter(|d| !d.trim().is_empty()).or(reversed_username),
            reversed_at: row.try_get::<Option<String>>("", "reversed_at")?.unwrap_or_default(),
            applied: row.try_get::<i64>("", "applied")? != 0,
        });
    }
    Ok(nodes)
}

/// Every count each count knows: those it follows and, through them, all they know (a walk over
/// the marks, safe against a list that loops).
fn known_by(nodes: &[CountNode]) -> HashMap<&str, HashSet<&str>> {
    let direct: HashMap<&str, &[String]> = nodes
        .iter()
        .map(|n| (n.count.count_sync_id.as_str(), n.follows.as_slice()))
        .collect();
    nodes
        .iter()
        .map(|n| {
            let mut known: HashSet<&str> = HashSet::new();
            let mut stack: Vec<&str> = n.follows.iter().map(String::as_str).collect();
            while let Some(id) = stack.pop() {
                if known.insert(id) {
                    if let Some(next) = direct.get(id) {
                        stack.extend(next.iter().map(String::as_str));
                    }
                }
            }
            (n.count.count_sync_id.as_str(), known)
        })
        .collect()
}

/// The mark of a count of an article and location, taken when its counted quantity and variance
/// are fixed (the count line entry, `controls::upsert_count_line`; the list,
/// `controls::record_location_count_in`): this PC's sync replica id and the JSON list of the counts
/// it follows — the counts this PC knows that no other known count follows, reversed ones included
/// (what they knew stays known), so the new count knows every count this PC knows.
///
/// Only counts whose correction is already in this PC's stock are followed, with everything they
/// know: a count line that arrived before its movement, or a count that knows one whose movement is
/// not here yet, is left out. A count never "knows" a correction the stock it is set against does
/// not include; the price is at worst a double count shown until the movement arrives (fail-safe).
/// Read it in the transaction that reads that stock.
///
/// The list also names the movements of every count of the pair posted twice that are in this PC's
/// stock (sync ids of `inventory_transactions`, not of count lines): the new count was set against a
/// stock holding them all, which closes that "comptage passé deux fois" ([`double_postings_of`]).
/// Ids that are not count lines are ignored by the double-count rule.
pub(crate) async fn count_mark(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<(String, String)> {
    let nodes = count_nodes(conn, article_id, location_id).await?;
    let known = known_by(&nodes);
    // Movements of a count posted twice that are in this PC's stock: a count that saw them all
    // closes that item ([`double_postings_of`]); they are named in the mark and count as applied.
    let doubled = double_posted_movement_ids(conn, article_id, location_id).await?;
    let mut applied: HashSet<&str> = nodes
        .iter()
        .filter(|n| n.applied)
        .map(|n| n.count.count_sync_id.as_str())
        .collect();
    applied.extend(doubled.iter().map(String::as_str));
    let eligible: Vec<&CountNode> = nodes
        .iter()
        .filter(|n| {
            let id = n.count.count_sync_id.as_str();
            applied.contains(id) && known.get(id).is_none_or(|k| k.iter().all(|a| applied.contains(a)))
        })
        .collect();
    let followed: HashSet<&str> = eligible
        .iter()
        .flat_map(|n| n.follows.iter().map(String::as_str))
        .collect();
    let mut follows: Vec<&str> = eligible
        .iter()
        .map(|n| n.count.count_sync_id.as_str())
        .filter(|id| !followed.contains(id))
        .collect();
    for id in &doubled {
        if !follows.contains(&id.as_str()) {
            follows.push(id);
        }
    }
    let follows_json = serde_json::to_string(&follows).map_err(|e| AppError::Internal(e.into()))?;
    Ok((crate::sync::queries::local_replica_id(conn).await?, follows_json))
}

/// The double counts of an article and location: those to verify, and those closed by a reversal.
/// The rule (CMMS Advisor ruling S1 and its SF2 ruling):
/// - a count takes part when it was made while the location stood below zero on its PC (the stock
///   on record when its variance was fixed, `system_qty`, kept on the line);
/// - two such counts are a double count when they come from different PCs (sync replica ids; a
///   line without one counts as another PC) and neither knows the other, even indirectly: knowing
///   goes through every count in between, so a count at zero or more (a zero-variance cycle count)
///   neither hides an earlier count nor links two counts that never knew each other;
/// - it is closed by a later count whose session is not reversed and that knows both, or by the
///   reversal of one of the two counts — the stock is then corrected once — noted "clos par
///   annulation du comptage <ref> (<qui>, <quand>)". This is the intended rule, not a limit; the
///   location's negative item, if the stock is still below zero, stays open on its own.
///
/// Only synchronised columns of the count lines and sessions are read, so every PC holding the same
/// lines gives the same answer. Counts are named oldest first.
struct DoubleCountState {
    /// The counts of every pair to verify (none: nothing to verify).
    open: Vec<ReconciliationCount>,
    /// For each reversed count that ended a double count: the reversal and the counts of its pairs.
    closed_by_reversal: Vec<(CountReversal, Vec<ReconciliationCount>)>,
}

async fn double_count_state(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<DoubleCountState> {
    let nodes = count_nodes(conn, article_id, location_id).await?;
    let known = known_by(&nodes);
    let knows = |who: &str, what: &str| known.get(who).is_some_and(|k| k.contains(what));
    let below: Vec<&CountNode> = nodes.iter().filter(|n| n.count.system_qty < -QTY_TOLERANCE).collect();
    // The reversal that ends a pair: the earlier one (time reversed, then line sync id).
    let ending = |x: &'_ CountNode, y: &'_ CountNode| -> Option<String> {
        let mut reversed: Vec<&CountNode> = [x, y].into_iter().filter(|n| n.reversed).collect();
        reversed.sort_by(|a, b| {
            (a.reversed_at.as_str(), a.count.count_sync_id.as_str())
                .cmp(&(b.reversed_at.as_str(), b.count.count_sync_id.as_str()))
        });
        reversed.first().map(|n| n.count.count_sync_id.clone())
    };
    let mut open: HashSet<&str> = HashSet::new();
    let mut closed: HashMap<String, HashSet<&str>> = HashMap::new();
    for (i, x) in below.iter().enumerate() {
        for y in &below[i + 1..] {
            let (xi, yi) = (x.count.count_sync_id.as_str(), y.count.count_sync_id.as_str());
            let same_pc = !x.replica_id.is_empty() && x.replica_id == y.replica_id;
            if same_pc || knows(xi, yi) || knows(yi, xi) {
                continue;
            }
            let verified = nodes.iter().any(|z| {
                let zi = z.count.count_sync_id.as_str();
                !z.reversed && knows(zi, xi) && knows(zi, yi)
            });
            if verified {
                continue;
            }
            match ending(x, y) {
                None => {
                    open.insert(xi);
                    open.insert(yi);
                }
                Some(by) => {
                    let members = closed.entry(by).or_default();
                    members.insert(xi);
                    members.insert(yi);
                }
            }
        }
    }
    let named = |ids: &HashSet<&str>| -> Vec<ReconciliationCount> {
        below
            .iter()
            .filter(|n| ids.contains(n.count.count_sync_id.as_str()))
            .map(|n| n.count.clone())
            .collect()
    };
    let mut closed_by_reversal: Vec<(CountReversal, Vec<ReconciliationCount>)> = closed
        .iter()
        .filter_map(|(by, ids)| {
            let n = nodes.iter().find(|n| n.count.count_sync_id == *by)?;
            Some((
                CountReversal {
                    count_sync_id: by.clone(),
                    session_code: n.count.session_code.clone(),
                    reversed_by: n.reversed_by.clone(),
                    reversed_at: n.reversed_at.clone(),
                },
                named(ids),
            ))
        })
        .collect();
    closed_by_reversal.sort_by(|(a, _), (b, _)| {
        (b.reversed_at.as_str(), b.count_sync_id.as_str()).cmp(&(a.reversed_at.as_str(), a.count_sync_id.as_str()))
    });
    Ok(DoubleCountState {
        open: named(&open),
        closed_by_reversal,
    })
}

/// The double count to verify of an article and location, if any ([`double_count_state`]).
pub(crate) async fn double_count_of(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<Option<Vec<ReconciliationCount>>> {
    let state = double_count_state(conn, article_id, location_id).await?;
    Ok((!state.open.is_empty()).then_some(state.open))
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
        if let Some(counts) = double_count_of(conn, article_id, location_id).await? {
            out.push((article_id, location_id, counts));
        }
    }
    Ok(out)
}

/// Count movements of the sessions whose count of an article and location is in the ledger more
/// than once (same type), as SQL over `inventory_transactions` filtered by article and location.
const DOUBLE_POSTED_GROUPS: &str = "SELECT source_id, movement_type FROM inventory_transactions
     WHERE source_type = 'COUNT_SESSION' AND movement_type IN ('COUNT_ADJUST', 'COUNT_REVERSAL')
       AND source_id IS NOT NULL AND entity_sync_id IS NOT NULL AND article_id = ? AND location_id = ?
     GROUP BY source_id, movement_type HAVING COUNT(*) > 1";

/// Audit events of a count whose quantity differed between two PCs (`inventory::count_sync`),
/// with the kept movement, for an article and location. Audit rows arrive from every PC: a row whose
/// details are not valid JSON is never parsed (`CASE`, so `json_extract` cannot fail the query), and
/// only count movements of a count session are joined.
const DIVERGENT_AUDITS: &str = "SELECT ae.target_id AS movement_sync_id, ae.details_json, t.source_id,
            t.movement_type, t.quantity, t.performed_at, ua.display_name, ua.username, cs.session_code,
            cs.created_at
     FROM audit_events ae
     JOIN inventory_transactions t ON t.entity_sync_id = ae.target_id
     JOIN inventory_count_sessions cs ON cs.id = t.source_id
     LEFT JOIN user_accounts ua ON ua.id = t.performed_by_id
     WHERE ae.action_code = 'inventory.count.posted_twice' AND ae.target_type = 'inventory_transactions'
       AND (CASE WHEN json_valid(ae.details_json) THEN json_extract(ae.details_json, '$.quantity_differs') END) = 1
       AND t.source_type = 'COUNT_SESSION' AND t.movement_type IN ('COUNT_ADJUST', 'COUNT_REVERSAL')
       AND t.article_id = ? AND t.location_id = ?
     ORDER BY cs.created_at, ae.target_id, ae.happened_at, ae.entity_sync_id";

/// Sync ids of every movement held here of the counts of an article and location posted more than
/// once, and of those whose quantity differed between two PCs (sorted, distinct).
async fn double_posted_movement_ids(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<Vec<String>> {
    let divergent = conn
        .query_all(sqlite(DIVERGENT_AUDITS, vec![article_id.into(), location_id.into()]))
        .await?;
    let mut ids: Vec<String> = divergent
        .iter()
        .map(|r| -> AppResult<String> { Ok(r.try_get::<String>("", "movement_sync_id")?) })
        .collect::<AppResult<_>>()?;
    let rows = conn
        .query_all(sqlite(
            &format!(
                "SELECT t.entity_sync_id FROM inventory_transactions t
                 JOIN ({DOUBLE_POSTED_GROUPS}) d ON d.source_id = t.source_id AND d.movement_type = t.movement_type
                 WHERE t.source_type = 'COUNT_SESSION' AND t.article_id = ? AND t.location_id = ?
                   AND t.entity_sync_id IS NOT NULL
                 ORDER BY t.entity_sync_id"
            ),
            vec![
                article_id.into(),
                location_id.into(),
                article_id.into(),
                location_id.into(),
            ],
        ))
        .await?;
    for row in &rows {
        ids.push(row.try_get::<String>("", "entity_sync_id")?);
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// One count of an article and location posted twice (or divergent), and the count that closed it,
/// if any.
struct DoublePosting {
    kind: &'static str,
    session_code: String,
    movement_type: String,
    movements: Vec<DoublePostedMovement>,
    closed_by: Option<ReconciliationCount>,
}

/// The counts of an article and location posted more than once (one per session and movement
/// type), oldest session first, then the divergent ones (one per movement, from their audit
/// events). Closed by a count of the pair whose session is not reversed and that knows every one of
/// their movements: a count entered on a PC holding them all ([`count_mark`] names them), directly
/// or through the counts it follows.
async fn double_postings_of(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<Vec<DoublePosting>> {
    let groups = conn
        .query_all(sqlite(
            &format!(
                "SELECT d.source_id, d.movement_type, cs.session_code, cs.created_at
                 FROM ({DOUBLE_POSTED_GROUPS}) d JOIN inventory_count_sessions cs ON cs.id = d.source_id
                 ORDER BY cs.created_at, cs.session_code, d.movement_type"
            ),
            vec![article_id.into(), location_id.into()],
        ))
        .await?;
    let divergent = conn
        .query_all(sqlite(DIVERGENT_AUDITS, vec![article_id.into(), location_id.into()]))
        .await?;
    if groups.is_empty() && divergent.is_empty() {
        return Ok(Vec::new());
    }
    let nodes = count_nodes(conn, article_id, location_id).await?;
    let known = known_by(&nodes);
    let closer = |ids: &[&str]| -> Option<ReconciliationCount> {
        nodes
            .iter()
            .find(|z| {
                !z.reversed
                    && known
                        .get(z.count.count_sync_id.as_str())
                        .is_some_and(|k| ids.iter().all(|id| k.contains(id)))
            })
            .map(|z| z.count.clone())
    };
    let mut out = Vec::with_capacity(groups.len() + divergent.len());
    for group in &groups {
        let session_id: i64 = group.try_get("", "source_id")?;
        let movement_type: String = group.try_get("", "movement_type")?;
        let rows = conn
            .query_all(sqlite(
                "SELECT t.entity_sync_id, t.performed_at, t.quantity, ua.display_name, ua.username
                 FROM inventory_transactions t
                 LEFT JOIN user_accounts ua ON ua.id = t.performed_by_id
                 WHERE t.source_type = 'COUNT_SESSION' AND t.source_id = ? AND t.movement_type = ?
                   AND t.article_id = ? AND t.location_id = ? AND t.entity_sync_id IS NOT NULL
                 ORDER BY t.performed_at, t.entity_sync_id",
                vec![
                    session_id.into(),
                    movement_type.clone().into(),
                    article_id.into(),
                    location_id.into(),
                ],
            ))
            .await?;
        let mut movements = Vec::with_capacity(rows.len());
        for row in &rows {
            let display: Option<String> = row.try_get("", "display_name")?;
            let username: Option<String> = row.try_get("", "username")?;
            movements.push(DoublePostedMovement {
                movement_sync_id: row.try_get("", "entity_sync_id")?,
                posted_by: display.filter(|d| !d.trim().is_empty()).or(username),
                posted_at: row.try_get("", "performed_at")?,
                quantity: row.try_get("", "quantity")?,
                device_code: None,
            });
        }
        let ids: Vec<&str> = movements.iter().map(|m| m.movement_sync_id.as_str()).collect();
        let closed_by = closer(&ids);
        out.push(DoublePosting {
            kind: DOUBLE_POSTING_POSTED_TWICE,
            session_code: group.try_get("", "session_code")?,
            movement_type,
            movements,
            closed_by,
        });
    }
    // Divergent counts: one item per movement, the version kept first, then each replaced version
    // named by its audit event (several when more than two PCs posted it).
    let mut divergent_items: Vec<DoublePosting> = Vec::new();
    for row in &divergent {
        let movement_sync_id: String = row.try_get("", "movement_sync_id")?;
        // An audit row this version cannot read (another detail shape, a damaged row received by
        // sync) is left out of the list, never allowed to break it (or a count's mark).
        let details: Option<serde_json::Value> = row
            .try_get::<Option<String>>("", "details_json")?
            .and_then(|json| serde_json::from_str(&json).ok());
        let replaced = details.as_ref().map(|d| &d["this_pc_posting"]);
        let (Some(details), Some(replaced), Some(replaced_qty)) = (
            details.as_ref(),
            replaced,
            replaced.and_then(|r| r["quantity"].as_f64()),
        ) else {
            tracing::warn!(
                movement_sync_id = %movement_sync_id,
                "negative_stock: unreadable divergent count audit skipped"
            );
            continue;
        };
        let replaced_version = DoublePostedMovement {
            movement_sync_id: movement_sync_id.clone(),
            posted_by: replaced["posted_by"].as_str().map(str::to_string),
            posted_at: replaced["posted_at"].as_str().unwrap_or_default().to_string(),
            quantity: replaced_qty,
            device_code: details["device_code"].as_str().map(str::to_string),
        };
        if let Some(item) = divergent_items.iter_mut().find(|d| {
            d.movements
                .first()
                .is_some_and(|m| m.movement_sync_id == movement_sync_id)
        }) {
            item.movements.push(replaced_version);
            continue;
        }
        let display: Option<String> = row.try_get("", "display_name")?;
        let username: Option<String> = row.try_get("", "username")?;
        let kept = DoublePostedMovement {
            movement_sync_id: movement_sync_id.clone(),
            posted_by: display.filter(|d| !d.trim().is_empty()).or(username),
            posted_at: row.try_get("", "performed_at")?,
            quantity: row.try_get("", "quantity")?,
            device_code: None,
        };
        divergent_items.push(DoublePosting {
            kind: DOUBLE_POSTING_DIVERGENT,
            session_code: row.try_get("", "session_code")?,
            movement_type: row.try_get("", "movement_type")?,
            movements: vec![kept, replaced_version],
            closed_by: closer(&[movement_sync_id.as_str()]),
        });
    }
    out.extend(divergent_items);
    Ok(out)
}

/// Whether an article and location has a count posted twice still to verify.
pub(crate) async fn double_posting_open(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<bool> {
    Ok(double_postings_of(conn, article_id, location_id)
        .await?
        .iter()
        .any(|d| d.closed_by.is_none()))
}

/// Every article and location with a count posted more than once or divergent (open or closed).
async fn double_posting_pairs(conn: &impl ConnectionTrait) -> AppResult<Vec<(i64, i64)>> {
    let rows = conn
        .query_all(sqlite(
            "SELECT DISTINCT article_id, location_id FROM (
                SELECT article_id, location_id FROM inventory_transactions
                WHERE source_type = 'COUNT_SESSION' AND movement_type IN ('COUNT_ADJUST', 'COUNT_REVERSAL')
                  AND source_id IS NOT NULL AND entity_sync_id IS NOT NULL
                GROUP BY source_id, article_id, location_id, movement_type HAVING COUNT(*) > 1
                UNION
                SELECT t.article_id, t.location_id FROM audit_events ae
                JOIN inventory_transactions t ON t.entity_sync_id = ae.target_id
                WHERE ae.action_code = 'inventory.count.posted_twice' AND ae.target_type = 'inventory_transactions'
                  AND (CASE WHEN json_valid(ae.details_json)
                       THEN json_extract(ae.details_json, '$.quantity_differs') END) = 1
                  AND t.source_type = 'COUNT_SESSION' AND t.movement_type IN ('COUNT_ADJUST', 'COUNT_REVERSAL'))
             ORDER BY article_id, location_id",
            vec![],
        ))
        .await?;
    rows.iter()
        .map(|r| -> AppResult<(i64, i64)> { Ok((r.try_get("", "article_id")?, r.try_get("", "location_id")?)) })
        .collect()
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
    let mut closed_double_counts = Vec::new();
    let pairs = db
        .query_all(sqlite(
            "SELECT DISTINCT article_id, location_id FROM inventory_count_lines
             WHERE count_follows_json IS NOT NULL
             ORDER BY article_id, location_id",
            vec![],
        ))
        .await?;
    for pair in pairs {
        let (article_id, location_id): (i64, i64) = (pair.try_get("", "article_id")?, pair.try_get("", "location_id")?);
        let state = double_count_state(db, article_id, location_id).await?;
        if state.open.is_empty() && (!include_closed || state.closed_by_reversal.is_empty()) {
            continue;
        }
        let Some(row) = db
            .query_one(sqlite(PAIR_SELECT, vec![location_id.into(), article_id.into()]))
            .await?
        else {
            continue;
        };
        let review = |counts: Vec<ReconciliationCount>,
                      closed_by_reversal: Option<CountReversal>|
         -> AppResult<DoubleCountReview> {
            Ok(DoubleCountReview {
                article_id,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                location_id,
                location_code: row.try_get("", "location_code")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                counts,
                closed_by_reversal,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
            })
        };
        if !state.open.is_empty() {
            double_counts.push(review(state.open, None)?);
        }
        if include_closed {
            for (reversal, counts) in state.closed_by_reversal {
                closed_double_counts.push(review(counts, Some(reversal))?);
            }
        }
    }
    closed_double_counts.sort_by(|a, b| {
        let at = |d: &DoubleCountReview| d.closed_by_reversal.as_ref().map(|r| r.reversed_at.clone());
        at(b).cmp(&at(a))
    });
    let mut double_postings = Vec::new();
    let mut closed_double_postings = Vec::new();
    for (article_id, location_id) in double_posting_pairs(db).await? {
        let postings = double_postings_of(db, article_id, location_id).await?;
        let Some(row) = db
            .query_one(sqlite(PAIR_SELECT, vec![location_id.into(), article_id.into()]))
            .await?
        else {
            continue;
        };
        for posting in postings {
            if posting.closed_by.is_some() && !include_closed {
                continue;
            }
            let variance_qty = posting.movements.first().map_or(0.0, |m| m.quantity);
            let excess_qty: f64 = if posting.kind == DOUBLE_POSTING_DIVERGENT {
                posting.movements.get(1).map_or(0.0, |m| m.quantity - variance_qty)
            } else {
                posting.movements.iter().skip(1).map(|m| m.quantity).sum()
            };
            let review = DoublePostedCountReview {
                kind: posting.kind.to_string(),
                article_id,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                location_id,
                location_code: row.try_get("", "location_code")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                session_code: posting.session_code,
                movement_type: posting.movement_type,
                movements: posting.movements,
                variance_qty,
                excess_qty,
                closed_by: posting.closed_by,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
            };
            if review.closed_by.is_some() {
                closed_double_postings.push(review);
            } else {
                double_postings.push(review);
            }
        }
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
        closed_double_counts,
        double_postings,
        closed_double_postings,
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
        double_posting_count: {
            let mut open = 0_i64;
            for (article_id, location_id) in double_posting_pairs(db).await? {
                open += i64::try_from(
                    double_postings_of(db, article_id, location_id)
                        .await?
                        .iter()
                        .filter(|d| d.closed_by.is_none())
                        .count(),
                )
                .unwrap_or(i64::MAX);
            }
            open
        },
        over_reserved_count: row.try_get("", "over_reserved_count")?,
    })
}

/// The most recent movement that took the on-hand stock of an article at a location from zero or
/// more to below zero, in the order every PC shares (time, then sync id), walked back from today's
/// level; `None` when the ledger shows it below zero from the start. Dates the notice (when the
/// shortage began); it does not name it, since a movement dated earlier that arrives late can move
/// it.
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
/// variance-reason list. The count is posted through the count workflow, marked as knowing every
/// count of the pair this PC knows ([`count_mark`]), and audited; the negative item closes since the
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
    let double_count = double_count_of(&tx, input.article_id, input.location_id)
        .await?
        .is_some();
    let double_posting = double_posting_open(&tx, input.article_id, input.location_id).await?;
    if item_id.is_none() && !double_count && !double_posting {
        return Err(refused(
            REFUSED_NOT_OPEN,
            "This location's stock is no longer to reconcile.",
            &[],
        ));
    }
    let (replica_id, follows_json) = count_mark(&tx, input.article_id, input.location_id).await?;
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
            mark: (&replica_id, &follows_json),
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
                "double_posting": double_posting,
                "follows_counts": serde_json::from_str::<serde_json::Value>(&follows_json).unwrap_or_default(),
                "replica_id": replica_id,
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
