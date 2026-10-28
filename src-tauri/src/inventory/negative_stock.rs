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
//! - one reservation issued beyond its reserved quantity, or one purchase-order line received
//!   beyond its ordered quantity, by movements of several PCs (AU-103): a "sur-sortie /
//!   sur-réception — à rapprocher" item, derived from the synchronised ledger and receipt lines
//!   (CMMS Advisor decision D12 (a), [`OverQuantityReview`]), closed the same way or, for an
//!   over-issue, by a return of the reservation back to its reserved quantity.
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
//! workflow (`controls::record_location_count_in`) and audited. It resolves a negative item, a
//! double count, a count posted twice and an over-issue or over-receipt alike. A later entry that
//! brings the stock back to zero or more also closes a negative item; how is kept
//! ([`closure_kind_of`], one kind for each stock entry of the ledger): a goods receipt or a transfer in is "régularisé par réception", a return naming its source
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
/// Refusal: the list count to approve is no longer waiting for its approval (approved, replaced by
/// a new count, or cancelled).
pub const REFUSED_COUNT_NOT_PENDING: &str = "NEGATIVE_STOCK_COUNT_NOT_PENDING";
/// Refusal: the stock on record changed since the count waiting for approval was made: its variance
/// would no longer bring the stock to the counted quantity; the location is counted again.
pub const REFUSED_COUNT_STALE: &str = "NEGATIVE_STOCK_COUNT_STALE";

/// Notification category of the notice to the counter of a list count whose approval was refused
/// because the stock on record moved since it (migration 259): count again.
pub const RECOUNT_CATEGORY: &str = "stock_count_recount";

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
    /// Who counted it: the line's counter (migration 259); for a line entered before, the session's
    /// poster.
    pub counted_by: Option<String>,
    /// Who approved its variance (the line's reviewer; on the list, the person counting), when it
    /// had one and the account is known here; a count without variance needs no approval and
    /// names none.
    pub approved_by: Option<String>,
    /// An auto-approbation (CMMS Advisor Q13, derived on read on every PC): flagged, or its variance
    /// approved by its own counter (`count_four_eyes::SELF_APPROVED_ON_READ_SQL`).
    pub self_approved: bool,
    pub device_code: String,
    pub counted_at: String,
    /// The stock on record the count was set against, on the PC that posted it.
    pub system_qty: f64,
    pub counted_qty: f64,
    pub variance_qty: f64,
}

/// A count recorded from this list above the four-eyes tolerance, waiting for a second person's
/// approval before it moves the stock ("comptage en attente d'approbation", D11 Q4a).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingReconciliationCount {
    /// The count line (approved with [`approve_pending_count`]).
    pub line_id: i64,
    pub row_version: i64,
    pub session_code: String,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub warehouse_code: String,
    /// Who counted it (never the one who approves it, unless the site opted out).
    pub counted_by_id: Option<i64>,
    pub counted_by: Option<String>,
    pub counted_at: String,
    pub system_qty: f64,
    pub counted_qty: f64,
    pub variance_qty: f64,
    pub variance_reason_code: Option<String>,
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

/// More issued against one reservation than it reserved (movements of several PCs).
pub const OVER_ISSUE: &str = "over_issue";
/// More received on one purchase-order line than it ordered (receipts of several PCs).
pub const OVER_RECEIPT: &str = "over_receipt";

/// One movement of an over-issue (an issue or a return of the reservation) or of an over-receipt
/// (a goods-receipt entry of the line at the location).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverQuantityMovement {
    pub movement_sync_id: String,
    /// `ISSUE`, `RETURN` or `GR_ACCEPT`.
    pub movement_type: String,
    /// The document it was recorded on (the goods-receipt number, the issue's reference), when any.
    pub document_ref: Option<String>,
    /// Who recorded it (the movement's author), when the account is known here.
    pub posted_by: Option<String>,
    pub posted_at: String,
    pub quantity: f64,
}

/// "Sur-sortie / sur-réception — à rapprocher" (CMMS Advisor decision D12 (a), AU-103): movements
/// are physical facts and are applied as received, so two PCs that each issue the last 5 of a
/// 5-unit reservation leave 10 issued against it, and two PCs that each receive the same PO line
/// leave it received twice (and the stock raised twice). Neither is refused; both are listed here.
///
/// - Over-issue (kind [`OVER_ISSUE`]): on one reservation, its issues net of its returns in the
///   ledger (`ISSUE` − `RETURN` linked to it), taken in the order every PC shares (time recorded,
///   a return before an issue of the same second, then sync id), exceed its reserved quantity at some point. Its location is the reservation's,
///   where every one of its movements is made.
/// - Over-receipt (kind [`OVER_RECEIPT`]): on one PO line, the quantity of its receipt lines
///   (`goods_receipt_lines`, one row per receipt, rejected part included as the receiving rule
///   counts it) exceeds its ordered quantity beyond [`RECEIVE_OVER_ORDERED_TOLERANCE`]. One item
///   per location the line was received into, each listing the stock entries made there
///   (`GR_ACCEPT` of those receipts).
///
/// [`RECEIVE_OVER_ORDERED_TOLERANCE`]: crate::inventory::procurement::RECEIVE_OVER_ORDERED_TOLERANCE
///
/// The reservation's `quantity_issued` and the line's `received_qty` are counters each PC keeps on
/// its row (two versions of one row are a sync conflict); this item never reads them. It is
/// derived from rows that are written once — the reservation's reserved quantity, the ledger, the
/// receipt lines and the line's ordered quantity — so every PC holding the same rows and movements
/// shows the same items, whatever order they arrived in. On one PC neither can happen: an issue is
/// refused beyond the reservation's remainder, a receipt beyond the ordered quantity.
///
/// Closed (CMMS Advisor ruling, 2026-10-23):
/// - by a return of the reservation (a `RETURN` linked to it: the work-order issue it returns)
///   that brings its issues net of returns back to its reserved quantity or less: "régularisé par
///   retour" ([`CLOSURE_RETURN`]), kept with the closed items with that return. A return that
///   names no reservation (a stock entry of the parts brought back) is not linked and changes
///   nothing; a later issue beyond the reserved quantity opens it again;
/// - otherwise, like a count posted twice, by a count of that article and location — from this
///   list, a count session or an inventory campaign alike — made on a PC holding every one of its
///   rows ([`count_mark`] names the movements and receipt lines, [`resolve_by_count`]): a movement
///   or receipt line that arrives after that count, or the reversal of that count, reopens it.
///
/// An over-receipt is never closed by a return (nothing reverses a receipt). Its receipts may have
/// been rejected in full: no stock moved, and the count confirms the stock as it stands.
///
/// Clock limit (over-issue): whether one is open depends only on the final net (all its issues
/// minus all its returns against the reserved quantity), a sum that no order changes, so a PC whose
/// clock is wrong never makes one wrongly open or wrongly closed. Only the history "régularisé par
/// retour" follows `performed_at`, the time each PC recorded: with a skewed clock it can show a
/// "closed by return" entry that did not happen (issue at t10, return stamped t30, issue at t20
/// reads as over then returned), or hide one that did, with the count that closed the item before
/// that return (issue stamped t100, then issue at t50 and return at t60 never reads as over). A
/// count made before still stays followed ([`count_mark`], [`held_rows_named`]).
///
/// Times are kept to the second. Movements recorded in the same second are read with the returns
/// first, then by sync id (Stock Lead ruling): one PC's issue, return and issue made within a
/// second never read as an over-issue. The price is a rare hidden history entry: two PCs' issues
/// and the return that brought them back, all in one second, read as never over (no item, and no
/// count closure shown). The status still depends on the final net only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverQuantityReview {
    /// [`OVER_ISSUE`] or [`OVER_RECEIPT`].
    pub kind: String,
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub location_id: i64,
    pub location_code: String,
    pub warehouse_code: String,
    /// Over-issue: the work order the reservation is for (either link kind), when it has one.
    pub work_order_code: Option<String>,
    /// Over-issue: the reservation's own reference (a reservation made without a work order).
    pub reservation_ref: Option<String>,
    /// Over-receipt: the purchase order of the line.
    pub purchase_order_number: Option<String>,
    /// Reserved (over-issue) or ordered (over-receipt) quantity.
    pub expected_qty: f64,
    /// Issued net of returns (over-issue; when closed by a return, at its highest before that
    /// return) or received on every receipt line (over-receipt).
    pub actual_qty: f64,
    /// `actual_qty − expected_qty`.
    pub excess_qty: f64,
    /// Oldest first (time recorded, then, for an over-issue, a return before an issue of the same
    /// second, then sync id).
    pub movements: Vec<OverQuantityMovement>,
    /// `None` while it is to reconcile; [`CLOSURE_RETURN`] or [`CLOSURE_COUNT`] once closed.
    pub closure_kind: Option<String>,
    /// The return that closed it ("régularisé par retour": who, when, the quantity returned).
    pub closed_by_return: Option<OverQuantityMovement>,
    /// The count that closed it: its session (list, count session or inventory campaign), who
    /// counted and who approved it.
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
    /// The over-issues and over-receipts to reconcile, then (when the closed items are asked for)
    /// those a return or a count closed.
    pub over_quantities: Vec<OverQuantityReview>,
    pub closed_over_quantities: Vec<OverQuantityReview>,
    pub over_reservations: Vec<OverReservationWarning>,
    /// Counts recorded here above the four-eyes tolerance, waiting for a second person: the item of
    /// their article and location shows "comptage en attente d'approbation".
    pub pending_counts: Vec<PendingReconciliationCount>,
    /// The stores' site opted out of four-eyes: the counter may approve a count waiting here
    /// (flagged "auto-approbation"); otherwise someone else approves it.
    pub self_approval_allowed: bool,
}

/// Counts for the sync centre, derived from the balances and the count sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegativeStockSummary {
    pub negative_count: i64,
    pub double_count_count: i64,
    /// Counts posted twice still to verify ([`DoublePostedCountReview`]).
    pub double_posting_count: i64,
    /// Over-issues and over-receipts still to reconcile ([`OverQuantityReview`]).
    pub over_quantity_count: i64,
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
    /// The count is above the four-eyes tolerance and waits for a second person's approval: nothing
    /// moved yet, the item stays open.
    pub pending_approval: bool,
    /// Approved by the person counting under the site's opt-out ("auto-approbation").
    pub self_approved: bool,
}

/// The approval of a list count waiting for it ([`approve_pending_count`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovePendingCountInput {
    pub line_id: i64,
    pub expected_row_version: i64,
    /// The approver's note (at least [`REVIEW_REASON_MIN_CHARS`] characters, as a count line's).
    pub reason: String,
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
            &format!(
                "SELECT cl.entity_sync_id, cs.session_code, cs.status, cl.count_replica_id, cl.count_follows_json,
                    cl.count_path, cs.posted_at, COALESCE(uc.display_name, ua.display_name) AS display_name,
                    CASE WHEN uc.id IS NOT NULL THEN uc.username ELSE ua.username END AS username,
                    {} AS self_approved, cl.system_qty, cl.counted_qty,
                    cl.variance_qty, cs.reversed_at, ra.display_name AS reversed_display, ra.username AS reversed_username,
                    aa.display_name AS approved_display, aa.username AS approved_username,
                    (ABS(cl.variance_qty) <= 1e-9
                     OR (EXISTS (SELECT 1 FROM inventory_transactions t WHERE t.id = cl.posted_transaction_id)
                         AND (cs.status <> 'reversed'
                              OR EXISTS (SELECT 1 FROM inventory_transactions t
                                         WHERE t.id = cl.reversed_transaction_id)))) AS applied
             FROM inventory_count_lines cl
             JOIN inventory_count_sessions cs ON cs.id = cl.session_id
             LEFT JOIN user_accounts ua ON ua.id = cs.posted_by_id
             LEFT JOIN user_accounts uc ON uc.id = cl.counted_by_id
             LEFT JOIN user_accounts ra ON ra.id = cs.reversed_by_id
             LEFT JOIN user_accounts aa ON aa.id = cl.approved_by_id
             WHERE cl.count_follows_json IS NOT NULL AND cl.entity_sync_id IS NOT NULL
               AND cs.status IN ('posted', 'reversed')
               AND cl.article_id = ? AND cl.location_id = ?
             ORDER BY cs.posted_at, cl.entity_sync_id",
                crate::inventory::count_four_eyes::SELF_APPROVED_ON_READ_SQL
            ),
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
        let approved_display: Option<String> = row.try_get("", "approved_display")?;
        let approved_username: Option<String> = row.try_get("", "approved_username")?;
        let variance_qty: f64 = row.try_get("", "variance_qty")?;
        // A count without variance needs no approval: none is named, whatever the line carries (a
        // count from the list records the person counting as its reviewer, with or without
        // variance; with one, that person is the approver).
        let approved_by = if variance_qty.abs() > QTY_TOLERANCE {
            approved_display.filter(|d| !d.trim().is_empty()).or(approved_username)
        } else {
            None
        };
        nodes.push(CountNode {
            count: ReconciliationCount {
                count_sync_id: row.try_get("", "entity_sync_id")?,
                session_code: row.try_get("", "session_code")?,
                path: row
                    .try_get::<Option<String>>("", "count_path")?
                    .unwrap_or_else(|| COUNT_PATH_COUNT_SESSION.to_string()),
                counted_by: display.filter(|d| !d.trim().is_empty()).or(username),
                approved_by,
                self_approved: row.try_get::<i64>("", "self_approved")? != 0,
                device_code: if replica_id.is_empty() {
                    String::new()
                } else {
                    crate::sync::device_code::device_code_for(&replica_id)
                },
                counted_at: row.try_get::<Option<String>>("", "posted_at")?.unwrap_or_default(),
                system_qty: row.try_get("", "system_qty")?,
                counted_qty: row.try_get("", "counted_qty")?,
                variance_qty,
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
/// It names likewise the movements and receipt lines of every over-issue and over-receipt of the
/// pair held here ([`over_quantities_of`]). Ids that are not count lines are ignored by the
/// double-count rule; one named by a known count is applied here once its row (a movement or a
/// receipt line) is held, whether or not it still makes an item, so a count stays followed after
/// a return or a later count changed what those rows make.
pub(crate) async fn count_mark(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<(String, String)> {
    let nodes = count_nodes(conn, article_id, location_id).await?;
    let known = known_by(&nodes);
    // Movements of a count posted twice, and rows of an over-issue or over-receipt, that are in
    // this PC's stock: a count that saw them all closes that item ([`double_postings_of`],
    // [`over_quantities_of`]); they are named in the mark and count as applied.
    let mut doubled = double_posted_movement_ids(conn, article_id, location_id).await?;
    let receipts = ReceiptCandidates::load(conn, Some(article_id)).await?;
    for found in over_quantities_found(conn, article_id, location_id, &receipts).await? {
        doubled.extend(found.held_ids);
    }
    doubled.sort();
    doubled.dedup();
    let held_named = held_rows_named(conn, &nodes).await?;
    let mut applied: HashSet<&str> = nodes
        .iter()
        .filter(|n| n.applied)
        .map(|n| n.count.count_sync_id.as_str())
        .collect();
    applied.extend(doubled.iter().map(String::as_str));
    applied.extend(held_named.iter().map(String::as_str));
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

/// The ids named by the counts of a pair that are not count lines and whose row is held here: a
/// stock movement (a count posted twice, an over-issue or over-receipt) or a goods-receipt line
/// (an over-receipt). Sorted.
async fn held_rows_named(conn: &impl ConnectionTrait, nodes: &[CountNode]) -> AppResult<Vec<String>> {
    let counts: HashSet<&str> = nodes.iter().map(|n| n.count.count_sync_id.as_str()).collect();
    let mut named: Vec<&str> = nodes
        .iter()
        .flat_map(|n| n.follows.iter().map(String::as_str))
        .filter(|id| !counts.contains(id))
        .collect();
    named.sort_unstable();
    named.dedup();
    if named.is_empty() {
        return Ok(Vec::new());
    }
    let named_json = serde_json::to_string(&named).map_err(|e| AppError::Internal(e.into()))?;
    let rows = conn
        .query_all(sqlite(
            "SELECT entity_sync_id FROM inventory_transactions
             WHERE entity_sync_id IN (SELECT value FROM json_each(?1))
             UNION
             SELECT entity_sync_id FROM goods_receipt_lines
             WHERE entity_sync_id IN (SELECT value FROM json_each(?1))
             ORDER BY 1",
            vec![named_json.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| -> AppResult<String> { Ok(r.try_get::<String>("", "entity_sync_id")?) })
        .collect()
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

/// The first count of the pair (oldest first) whose session is not reversed and that knows every
/// one of `ids`, directly or through the counts it follows: the count that closes an item made of
/// those rows. `None` when `ids` is empty (no count closes an item it never saw).
fn closing_count(
    nodes: &[CountNode],
    known: &HashMap<&str, HashSet<&str>>,
    ids: &[&str],
) -> Option<ReconciliationCount> {
    if ids.is_empty() {
        return None;
    }
    nodes
        .iter()
        .find(|z| {
            !z.reversed
                && known
                    .get(z.count.count_sync_id.as_str())
                    .is_some_and(|k| ids.iter().all(|id| k.contains(id)))
        })
        .map(|z| z.count.clone())
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
    let closer = |ids: &[&str]| closing_count(&nodes, &known, ids);
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

/// An over-issue or over-receipt found at an article and location ([`over_quantities_found`]).
struct OverQuantity {
    kind: &'static str,
    work_order_code: Option<String>,
    reservation_ref: Option<String>,
    purchase_order_number: Option<String>,
    expected_qty: f64,
    actual_qty: f64,
    movements: Vec<OverQuantityMovement>,
    /// Sync ids of the rows it is made of that are held here (its movements; for an over-receipt
    /// also its receipt lines at the location): a count knowing them all closes it.
    held_ids: Vec<String>,
    /// Over-issue: the return of the reservation that brought it back to its reserved quantity or
    /// less, when it is back there ("régularisé par retour").
    closing_return: Option<OverQuantityMovement>,
}

/// Reservations of an article and location whose issues in the ledger, returns left aside, exceed
/// their reserved quantity: the issues net of returns can only have gone beyond it then (SQL
/// prefilter, without tolerance; the exact rule is applied in Rust). The reservation link is
/// indexed (`idx_inventory_transactions_reservation`).
const OVER_ISSUE_CANDIDATES: &str = "SELECT r.id, r.quantity_reserved, r.source_ref
     FROM stock_reservations r
     WHERE r.article_id = ? AND r.location_id = ?
       AND (SELECT COALESCE(SUM(t.quantity), 0)
            FROM inventory_transactions t
            WHERE t.reservation_id = r.id AND t.movement_type = 'ISSUE'
              AND t.article_id = r.article_id AND t.location_id = r.location_id
              AND t.entity_sync_id IS NOT NULL) > r.quantity_reserved
     ORDER BY r.entity_sync_id, r.id";

/// One PO line received into one location beyond its ordered quantity (SQL prefilter, without
/// tolerance; the exact rule is applied in Rust, [`over_quantities_found`]).
struct ReceiptCandidate {
    article_id: i64,
    location_id: i64,
    po_line_id: i64,
    ordered_qty: f64,
    po_number: String,
}

/// One receipt line of a candidate PO line (synchronised or not).
struct CandidateReceiptLine {
    sync_id: Option<String>,
    location_id: i64,
    received_qty: f64,
    goods_receipt_id: i64,
}

/// The over-receipt candidates and their receipt lines, read once per call ([`list_reviews`],
/// [`summary`]) or once per article ([`count_mark`], [`over_quantity_open`]), never once per
/// article and location: `goods_receipt_lines` has no index on its PO line (a later migration can
/// add one on `goods_receipt_lines(po_line_id)`), so each read of them is a pass over the whole
/// table. Over-issue candidates never read them.
struct ReceiptCandidates {
    /// In the order of the items (PO number, line sync id, line id).
    candidates: Vec<ReceiptCandidate>,
    lines: HashMap<i64, Vec<CandidateReceiptLine>>,
}

impl ReceiptCandidates {
    /// The candidates of every article (`None`) or of one.
    async fn load(conn: &impl ConnectionTrait, article_id: Option<i64>) -> AppResult<Self> {
        // Every PO line (of the article) and location it was received into, with what its
        // synchronised receipt lines received in all (`line_received`, over every location): one
        // pass over the receipt lines, grouped once. The article filter keeps or drops whole PO
        // lines, so a line's total is the same either way.
        let (line_filter, article_filter, values) = match article_id {
            Some(id) => (
                "WHERE po_line_id IN (SELECT id FROM purchase_order_lines WHERE article_id = ?1)",
                "AND pol.article_id = ?1",
                vec![id.into()],
            ),
            None => ("", "", vec![]),
        };
        let candidates = conn
            .query_all(sqlite(
                &format!(
                    "SELECT pol.article_id, rc.location_id, pol.id, pol.ordered_qty, po.po_number
                     FROM (SELECT po_line_id, location_id,
                                  SUM(SUM(CASE WHEN entity_sync_id IS NOT NULL THEN received_qty ELSE 0 END))
                                      OVER (PARTITION BY po_line_id) AS line_received
                           FROM goods_receipt_lines {line_filter}
                           GROUP BY po_line_id, location_id) rc
                     JOIN purchase_order_lines pol ON pol.id = rc.po_line_id
                     JOIN purchase_orders po ON po.id = pol.purchase_order_id
                     WHERE rc.line_received > pol.ordered_qty {article_filter}
                     ORDER BY po.po_number, pol.entity_sync_id, pol.id"
                ),
                values,
            ))
            .await?
            .iter()
            .map(|row| -> AppResult<ReceiptCandidate> {
                Ok(ReceiptCandidate {
                    article_id: row.try_get("", "article_id")?,
                    location_id: row.try_get("", "location_id")?,
                    po_line_id: row.try_get("", "id")?,
                    ordered_qty: row.try_get("", "ordered_qty")?,
                    po_number: row.try_get("", "po_number")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let mut lines: HashMap<i64, Vec<CandidateReceiptLine>> = HashMap::new();
        if !candidates.is_empty() {
            let mut po_line_ids: Vec<i64> = candidates.iter().map(|c| c.po_line_id).collect();
            po_line_ids.sort_unstable();
            po_line_ids.dedup();
            let ids_json = serde_json::to_string(&po_line_ids).map_err(|e| AppError::Internal(e.into()))?;
            for row in conn
                .query_all(sqlite(
                    "SELECT po_line_id, entity_sync_id, location_id, received_qty, goods_receipt_id
                     FROM goods_receipt_lines WHERE po_line_id IN (SELECT value FROM json_each(?))",
                    vec![ids_json.into()],
                ))
                .await?
            {
                lines
                    .entry(row.try_get("", "po_line_id")?)
                    .or_default()
                    .push(CandidateReceiptLine {
                        sync_id: row.try_get("", "entity_sync_id")?,
                        location_id: row.try_get("", "location_id")?,
                        received_qty: row.try_get("", "received_qty")?,
                        goods_receipt_id: row.try_get("", "goods_receipt_id")?,
                    });
            }
        }
        Ok(Self { candidates, lines })
    }

    /// The candidates of an article and location, in the order of the items.
    fn at(&self, article_id: i64, location_id: i64) -> impl Iterator<Item = &ReceiptCandidate> {
        self.candidates
            .iter()
            .filter(move |c| c.article_id == article_id && c.location_id == location_id)
    }
}

/// Sum of quantities in sync-id order, so the floating-point result is the same on every PC.
fn sum_in_sync_order(mut rows: Vec<(String, f64)>) -> f64 {
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    rows.iter().map(|(_, q)| q).sum()
}

fn over_movement(row: &QueryResult) -> AppResult<OverQuantityMovement> {
    let display: Option<String> = row.try_get("", "display_name")?;
    let username: Option<String> = row.try_get("", "username")?;
    Ok(OverQuantityMovement {
        movement_sync_id: row.try_get("", "entity_sync_id")?,
        movement_type: row.try_get("", "movement_type")?,
        document_ref: row
            .try_get::<Option<String>>("", "source_ref")?
            .filter(|r| !r.trim().is_empty()),
        posted_by: display.filter(|d| !d.trim().is_empty()).or(username),
        posted_at: row.try_get::<Option<String>>("", "performed_at")?.unwrap_or_default(),
        quantity: row.try_get("", "quantity")?,
    })
}

/// The order of a reservation's issues and returns, the same on every PC: time recorded, then a
/// return before an issue recorded in the same second (Stock Lead ruling: times are kept to the
/// second, so one PC's issue, return and issue made within one second would otherwise read in
/// sync-id order and could show a false "régularisé par retour"), then sync id. The SQL of
/// [`over_quantities_found`] lists the movements in this order too.
fn over_issue_order(a: &OverQuantityMovement, b: &OverQuantityMovement) -> std::cmp::Ordering {
    let issue_last = |m: &OverQuantityMovement| m.movement_type == "ISSUE";
    a.posted_at
        .cmp(&b.posted_at)
        .then(issue_last(a).cmp(&issue_last(b)))
        .then(a.movement_sync_id.cmp(&b.movement_sync_id))
}

/// What the issues and returns of one reservation make ([`over_issue_walk`]).
pub(super) struct OverIssueWalk {
    /// Issued net of returns now, or at its highest before the return that closed it.
    pub(super) actual_qty: f64,
    pub(super) closing_return: Option<OverQuantityMovement>,
}

/// Walks the issues and returns of a reservation in the order every PC shares ([`over_issue_order`]:
/// time recorded, a return before an issue of the same time, then sync id), so the floating-point
/// sums are the same everywhere: `None` when the issues net of returns never went beyond
/// `reserved`. Otherwise the over-issue is open while they are still beyond it; once a return
/// brought them back to it or less, it is closed by the last such return ("régularisé par retour"),
/// unless a later issue took them beyond it again.
///
/// Open or not depends on the final net only (the order never changes a sum's sign against
/// `reserved`); the order, taken from each PC's clock, decides only whether a closed-by-return
/// entry is shown: a skewed clock can show a false one or hide a real one (and its count closure
/// with it), never give a wrong open status ([`OverQuantityReview`], clock limit).
pub(super) fn over_issue_walk(movements: &[OverQuantityMovement], reserved: f64) -> Option<OverIssueWalk> {
    let mut ordered: Vec<&OverQuantityMovement> = movements.iter().collect();
    ordered.sort_by(|a, b| over_issue_order(a, b));
    let mut net = 0.0_f64;
    let mut over = false;
    let mut peak = 0.0_f64;
    let mut found: Option<OverIssueWalk> = None;
    for m in ordered {
        net += if m.movement_type == "ISSUE" {
            m.quantity
        } else {
            -m.quantity
        };
        if net > reserved + QTY_TOLERANCE {
            peak = if over { peak.max(net) } else { net };
            over = true;
            found = Some(OverIssueWalk {
                actual_qty: net,
                closing_return: None,
            });
        } else if over {
            // Only a return brings the net issued down.
            over = false;
            found = Some(OverIssueWalk {
                actual_qty: peak,
                closing_return: Some(m.clone()),
            });
        }
    }
    found
}

/// The over-issues and over-receipts of an article and location, from the rows held here: the
/// reservations of the pair issued beyond their reserved quantity, then the PO lines received into
/// the location beyond their ordered quantity ([`OverQuantityReview`]), from `receipts` (read once by
/// the caller, [`ReceiptCandidates`]).
async fn over_quantities_found(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
    receipts: &ReceiptCandidates,
) -> AppResult<Vec<OverQuantity>> {
    let mut out = Vec::new();
    let wo_sources = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL;
    for reservation in conn
        .query_all(sqlite(
            OVER_ISSUE_CANDIDATES,
            vec![article_id.into(), location_id.into()],
        ))
        .await?
    {
        let reservation_id: i64 = reservation.try_get("", "id")?;
        let reserved: f64 = reservation.try_get("", "quantity_reserved")?;
        let rows = conn
            .query_all(sqlite(
                "SELECT t.entity_sync_id, t.movement_type, t.source_ref, t.performed_at, t.quantity,
                        ua.display_name, ua.username
                 FROM inventory_transactions t
                 LEFT JOIN user_accounts ua ON ua.id = t.performed_by_id
                 WHERE t.reservation_id = ? AND t.movement_type IN ('ISSUE', 'RETURN')
                   AND t.article_id = ? AND t.location_id = ? AND t.entity_sync_id IS NOT NULL
                 ORDER BY COALESCE(t.performed_at, ''), CASE t.movement_type WHEN 'ISSUE' THEN 1 ELSE 0 END,
                          t.entity_sync_id",
                vec![reservation_id.into(), article_id.into(), location_id.into()],
            ))
            .await?;
        let movements = rows.iter().map(over_movement).collect::<AppResult<Vec<_>>>()?;
        let Some(walk) = over_issue_walk(&movements, reserved) else {
            continue;
        };
        let work_order_code = conn
            .query_one(sqlite(
                &format!(
                    "SELECT wo.code FROM work_orders wo
                     WHERE wo.id = (SELECT r.source_id FROM stock_reservations r
                                    WHERE r.id = ?1 AND r.source_type IN ({wo_sources}))
                        OR wo.id IN (SELECT wop.work_order_id FROM work_order_parts wop WHERE wop.reservation_id = ?1)
                     ORDER BY wo.code LIMIT 1"
                ),
                vec![reservation_id.into()],
            ))
            .await?
            .map(|row| row.try_get::<String>("", "code"))
            .transpose()?;
        out.push(OverQuantity {
            kind: OVER_ISSUE,
            work_order_code,
            reservation_ref: reservation
                .try_get::<Option<String>>("", "source_ref")?
                .filter(|r| !r.trim().is_empty()),
            purchase_order_number: None,
            expected_qty: reserved,
            actual_qty: walk.actual_qty,
            held_ids: movements.iter().map(|m| m.movement_sync_id.clone()).collect(),
            movements,
            closing_return: walk.closing_return,
        });
    }
    for line in receipts.at(article_id, location_id) {
        let ordered = line.ordered_qty;
        let receipt_lines = receipts.lines.get(&line.po_line_id).map_or(&[][..], Vec::as_slice);
        let mut received = Vec::with_capacity(receipt_lines.len());
        let mut held_ids = Vec::new();
        let mut goods_receipt_ids = Vec::new();
        for row in receipt_lines {
            if row.location_id == location_id {
                goods_receipt_ids.push(row.goods_receipt_id);
            }
            let Some(sync_id) = &row.sync_id else {
                continue;
            };
            if row.location_id == location_id {
                held_ids.push(sync_id.clone());
            }
            received.push((sync_id.clone(), row.received_qty));
        }
        let received = sum_in_sync_order(received);
        // The receiving rule's own margin: what one PC may receive is never an over-receipt.
        if received <= ordered + crate::inventory::procurement::RECEIVE_OVER_ORDERED_TOLERANCE {
            continue;
        }
        goods_receipt_ids.sort_unstable();
        goods_receipt_ids.dedup();
        let goods_receipts_json =
            serde_json::to_string(&goods_receipt_ids).map_err(|e| AppError::Internal(e.into()))?;
        // The stock entries of this line's receipts at the location (a receipt movement names its
        // goods receipt, not its line: the receipt's line of this article and location is the one).
        let rows = conn
            .query_all(sqlite(
                "SELECT t.entity_sync_id, t.movement_type, t.source_ref, t.performed_at, t.quantity,
                        ua.display_name, ua.username
                 FROM inventory_transactions t
                 LEFT JOIN user_accounts ua ON ua.id = t.performed_by_id
                 WHERE t.movement_type = 'GR_ACCEPT' AND t.source_type = 'PO_GR'
                   AND t.article_id = ? AND t.location_id = ? AND t.entity_sync_id IS NOT NULL
                   AND t.source_id IN (SELECT value FROM json_each(?))
                 ORDER BY t.performed_at, t.entity_sync_id",
                vec![article_id.into(), location_id.into(), goods_receipts_json.into()],
            ))
            .await?;
        let movements = rows.iter().map(over_movement).collect::<AppResult<Vec<_>>>()?;
        held_ids.extend(movements.iter().map(|m| m.movement_sync_id.clone()));
        held_ids.sort();
        out.push(OverQuantity {
            kind: OVER_RECEIPT,
            work_order_code: None,
            reservation_ref: None,
            purchase_order_number: Some(line.po_number.clone()),
            expected_qty: ordered,
            actual_qty: received,
            movements,
            held_ids,
            closing_return: None,
        });
    }
    Ok(out)
}

/// The over-issues and over-receipts of an article and location, each with the count that closed
/// it: a count of the pair whose session is not reversed and that knows every one of its rows
/// ([`closing_count`], [`count_mark`]). An over-issue closed by a return names no count, even when
/// a count closed it before that return: the return is the closure shown, the earlier count stays
/// in the count history (and stays followed, [`held_rows_named`]); a count made after the return
/// does not reopen or re-close it.
async fn over_quantities_of(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
    receipts: &ReceiptCandidates,
) -> AppResult<Vec<(OverQuantity, Option<ReconciliationCount>)>> {
    let found = over_quantities_found(conn, article_id, location_id, receipts).await?;
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let nodes = count_nodes(conn, article_id, location_id).await?;
    let known = known_by(&nodes);
    Ok(found
        .into_iter()
        .map(|item| {
            let ids: Vec<&str> = item.held_ids.iter().map(String::as_str).collect();
            let closed_by = if item.closing_return.is_some() {
                None
            } else {
                closing_count(&nodes, &known, &ids)
            };
            (item, closed_by)
        })
        .collect())
}

/// Whether an article and location has an over-issue or over-receipt still to reconcile.
pub(crate) async fn over_quantity_open(
    conn: &impl ConnectionTrait,
    article_id: i64,
    location_id: i64,
) -> AppResult<bool> {
    let receipts = ReceiptCandidates::load(conn, Some(article_id)).await?;
    Ok(over_quantities_of(conn, article_id, location_id, &receipts)
        .await?
        .iter()
        .any(|(item, closed_by)| closed_by.is_none() && item.closing_return.is_none()))
}

/// Every article and location with an over-issue or over-receipt (open or closed; prefilter, the
/// same conditions as [`OVER_ISSUE_CANDIDATES`] and [`ReceiptCandidates`], whose candidates the
/// caller read once), in article then location order.
async fn over_quantity_pairs(conn: &impl ConnectionTrait, receipts: &ReceiptCandidates) -> AppResult<Vec<(i64, i64)>> {
    let rows = conn
        .query_all(sqlite(
            "SELECT DISTINCT r.article_id, r.location_id FROM stock_reservations r
             JOIN inventory_transactions t ON t.reservation_id = r.id
             WHERE t.movement_type = 'ISSUE' AND t.entity_sync_id IS NOT NULL
               AND t.article_id = r.article_id AND t.location_id = r.location_id
             GROUP BY r.id
             HAVING SUM(t.quantity) > r.quantity_reserved",
            vec![],
        ))
        .await?;
    let mut pairs = rows
        .iter()
        .map(|r| -> AppResult<(i64, i64)> { Ok((r.try_get("", "article_id")?, r.try_get("", "location_id")?)) })
        .collect::<AppResult<Vec<_>>>()?;
    pairs.extend(receipts.candidates.iter().map(|c| (c.article_id, c.location_id)));
    pairs.sort_unstable();
    pairs.dedup();
    Ok(pairs)
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
    let mut over_quantities = Vec::new();
    let mut closed_over_quantities = Vec::new();
    let receipts = ReceiptCandidates::load(db, None).await?;
    for (article_id, location_id) in over_quantity_pairs(db, &receipts).await? {
        let found = over_quantities_of(db, article_id, location_id, &receipts).await?;
        if found.is_empty() {
            continue;
        }
        let Some(row) = db
            .query_one(sqlite(PAIR_SELECT, vec![location_id.into(), article_id.into()]))
            .await?
        else {
            continue;
        };
        for (item, closed_by) in found {
            let closure_kind = if item.closing_return.is_some() {
                Some(CLOSURE_RETURN)
            } else {
                closed_by.as_ref().map(|_| CLOSURE_COUNT)
            };
            if closure_kind.is_some() && !include_closed {
                continue;
            }
            let review = OverQuantityReview {
                kind: item.kind.to_string(),
                article_id,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                location_id,
                location_code: row.try_get("", "location_code")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                work_order_code: item.work_order_code,
                reservation_ref: item.reservation_ref,
                purchase_order_number: item.purchase_order_number,
                expected_qty: item.expected_qty,
                actual_qty: item.actual_qty,
                excess_qty: item.actual_qty - item.expected_qty,
                movements: item.movements,
                closure_kind: closure_kind.map(str::to_string),
                closed_by_return: item.closing_return,
                closed_by,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
            };
            if review.closure_kind.is_some() {
                closed_over_quantities.push(review);
            } else {
                over_quantities.push(review);
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
    let pending_counts = db
        .query_all(sqlite(
            "SELECT cl.id AS line_id, cl.row_version, cs.session_code, cl.article_id, a.article_code, a.article_name,
                    cl.location_id, sl.code AS location_code, w.code AS warehouse_code, cl.counted_by_id,
                    COALESCE(NULLIF(TRIM(uc.display_name), ''), uc.username) AS counted_by, cs.created_at AS counted_at,
                    cl.system_qty, cl.counted_qty, cl.variance_qty, cl.variance_reason_code
             FROM inventory_count_lines cl
             JOIN inventory_count_sessions cs ON cs.id = cl.session_id
             JOIN articles a ON a.id = cl.article_id
             JOIN stock_locations sl ON sl.id = cl.location_id
             JOIN warehouses w ON w.id = sl.warehouse_id
             LEFT JOIN user_accounts uc ON uc.id = cl.counted_by_id
             WHERE cl.count_path = ? AND cl.approved_by_id IS NULL AND cs.status = 'submitted'
             ORDER BY cs.created_at, cl.id",
            vec![COUNT_PATH_RECONCILIATION.into()],
        ))
        .await?
        .iter()
        .map(|row| -> AppResult<PendingReconciliationCount> {
            Ok(PendingReconciliationCount {
                line_id: row.try_get("", "line_id")?,
                row_version: row.try_get("", "row_version")?,
                session_code: row.try_get("", "session_code")?,
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                counted_by_id: row.try_get("", "counted_by_id")?,
                counted_by: row.try_get("", "counted_by")?,
                counted_at: row.try_get("", "counted_at")?,
                system_qty: row.try_get("", "system_qty")?,
                counted_qty: row.try_get("", "counted_qty")?,
                variance_qty: row.try_get("", "variance_qty")?,
                variance_reason_code: row.try_get("", "variance_reason_code")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(NegativeStockReviewList {
        items,
        double_counts,
        closed_double_counts,
        double_postings,
        closed_double_postings,
        over_quantities,
        closed_over_quantities,
        over_reservations,
        pending_counts,
        self_approval_allowed: crate::inventory::count_four_eyes::store_rule(db).await?.rule
            == crate::reliability::study::decisions::SelfApprovalRule::Allowed,
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
        over_quantity_count: {
            let mut open = 0_i64;
            let receipts = ReceiptCandidates::load(db, None).await?;
            for (article_id, location_id) in over_quantity_pairs(db, &receipts).await? {
                open += i64::try_from(
                    over_quantities_of(db, article_id, location_id, &receipts)
                        .await?
                        .iter()
                        .filter(|(item, closed_by)| closed_by.is_none() && item.closing_return.is_none())
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
/// or a count posted twice to verify, an over-issue or over-receipt, or several — by a stock count recorded there by `user_id`: needs `inv.manage`
/// (plant-wide: stores have no site yet, as for stock adjustments), a reason of at least
/// [`REVIEW_REASON_MIN_CHARS`] characters, the quantity counted (zero or more) and a code of the
/// variance-reason list. The count is recorded through the count workflow, marked as knowing every
/// count of the pair this PC knows ([`count_mark`]), and audited. Within the four-eyes tolerance (or
/// above it under the opt-out of the organisation's only site, flagged "auto-approbation") it is
/// posted at once and the negative item closes since the stock is now the counted quantity; above
/// it elsewhere it is saved as counted, moves nothing and waits for a second person
/// ([`approve_pending_count`], D11 Q4a).
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
    let over_quantity = over_quantity_open(&tx, input.article_id, input.location_id).await?;
    if item_id.is_none() && !double_count && !double_posting && !over_quantity {
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
                "summary": if posted.pending_approval {
                    "Count recorded from the stock to reconcile, waiting for a second person's approval (nothing posted)"
                } else {
                    "Stock to reconcile resolved by a count"
                },
                "article_id": input.article_id,
                "location_id": input.location_id,
                "negative_review_id": item_id,
                "double_count": double_count,
                "double_posting": double_posting,
                "over_quantity": over_quantity,
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
                "four_eyes_tolerance": posted.tolerance_json,
                "pending_approval": posted.pending_approval,
                "self_approved": posted.self_approved,
                "replaced_session_codes": posted.replaced_sessions,
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
        pending_approval: posted.pending_approval,
        self_approved: posted.self_approved,
    })
}

/// Approves a count recorded from this list above the four-eyes tolerance and waiting for it, by
/// `user_id` (D11 Q4a): needs `inv.manage` (the right of a count line's approval), a note of at least
/// [`REVIEW_REASON_MIN_CHARS`] characters, someone other than the counter (unless the
/// organisation's only site opted out: then flagged "auto-approbation"), and the stock on record
/// still the one the count was set against (otherwise refused: the location is counted again). In
/// one transaction the count's movement is posted, its session posted and the item of the pair
/// closed as today ([`reconcile_pair_in`]); audited.
pub async fn approve_pending_count(
    db: &DatabaseConnection,
    user_id: i32,
    input: ApprovePendingCountInput,
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
            format!("Give the reason of the approval (at least {REVIEW_REASON_MIN_CHARS} characters)."),
            &[("min", REVIEW_REASON_MIN_CHARS.to_string())],
        ));
    }
    let tx = db.begin().await?;
    let Some(pending) = crate::inventory::controls::pending_location_count_in(&tx, input.line_id).await? else {
        return Err(refused(
            REFUSED_COUNT_NOT_PENDING,
            "This count is no longer waiting for its approval: refresh the list.",
            &[],
        ));
    };
    if pending.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: count line row_version mismatch.".to_string(),
        ]));
    }
    let actor_id = i64::from(user_id);
    let self_approved = crate::inventory::count_four_eyes::check_approval(
        &tx,
        true,
        pending.is_critical,
        pending.counted_by_id,
        actor_id,
    )
    .await?;
    // The stock on record, whatever moved it: a movement made here or received from another PC
    // (applied to the balance when it arrives).
    let (_, on_hand) = crate::inventory::controls::get_balance(&tx, pending.article_id, pending.location_id).await?;
    if (on_hand - pending.system_qty).abs() > QTY_TOLERANCE {
        tx.rollback().await?;
        // The counter is told to count again (the approver sees the refusal).
        notify_recount(db, &pending, on_hand).await;
        return Err(refused(
            REFUSED_COUNT_STALE,
            format!(
                "The stock of this location moved since the count {} (from {} to {on_hand}): count it again.",
                pending.session_code, pending.system_qty
            ),
            &[("session", pending.session_code.clone())],
        ));
    }
    // The negative item open when the count is approved (none when only a double count, a count
    // posted twice or an over-quantity was to reconcile): the posting closes it.
    reconcile_pair_in(&tx, pending.article_id, pending.location_id).await?;
    let item_id = tx
        .query_one(sqlite(
            "SELECT id FROM stock_negative_reviews WHERE article_id = ? AND location_id = ? AND status = 'open'",
            vec![pending.article_id.into(), pending.location_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "id"))
        .transpose()?;
    let posted =
        crate::inventory::controls::approve_location_count_in(&tx, &pending, actor_id, reason, self_approved).await?;
    reconcile_pair_in(&tx, pending.article_id, pending.location_id).await?;
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "inventory.negative_stock.count_approved".to_string(),
            target_type: Some("stock_location_article".to_string()),
            target_id: Some(format!("{}:{}", pending.article_id, pending.location_id)),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": if self_approved {
                    "Count waiting for approval self-approved (site opt-out) and posted"
                } else {
                    "Count waiting for approval approved by a second person and posted"
                },
                "article_id": pending.article_id,
                "location_id": pending.location_id,
                "counted_by_id": pending.counted_by_id,
                "self_approved": self_approved,
                "system_qty": pending.system_qty,
                "counted_qty": pending.counted_qty,
                "variance_qty": pending.variance_qty,
                "reason": reason,
                "count_session_id": posted.session_id,
                "count_session_code": posted.session_code,
                "count_line_id": posted.line_id,
                "transaction_id": posted.transaction_id,
                "negative_review_id": item_id,
                "four_eyes_tolerance": posted.tolerance_json,
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
        counted_qty: pending.counted_qty,
        variance_qty: posted.variance_qty,
        review,
        pending_approval: false,
        self_approved,
    })
}

/// The in-app notice to the counter of a list count whose approval was refused because the stock on
/// record moved since the count: the article, the location and why, to count again (fire-and-log:
/// the refusal stands whatever happens to the notice).
async fn notify_recount(
    db: &DatabaseConnection,
    pending: &crate::inventory::controls::PendingLocationCount,
    on_hand: f64,
) {
    let names = db
        .query_one(sqlite(
            "SELECT a.article_code, a.article_name, w.code AS warehouse_code, sl.code AS location_code
             FROM articles a, stock_locations sl JOIN warehouses w ON w.id = sl.warehouse_id
             WHERE a.id = ? AND sl.id = ?",
            vec![pending.article_id.into(), pending.location_id.into()],
        ))
        .await;
    let row = match names {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(error = %err, line_id = pending.line_id, "recount notice: names not read");
            return;
        }
    };
    let text = |col: &str| row.try_get::<String>("", col).unwrap_or_default();
    let (code, name) = (text("article_code"), text("article_name"));
    let line_sync_id = match db
        .query_one(sqlite(
            "SELECT COALESCE(NULLIF(TRIM(entity_sync_id), ''), CAST(id AS TEXT)) AS sid FROM inventory_count_lines WHERE id = ?",
            vec![pending.line_id.into()],
        ))
        .await
    {
        Ok(Some(r)) => r.try_get::<String>("", "sid").unwrap_or_default(),
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(error = %err, line_id = pending.line_id, "recount notice: line not read");
            return;
        }
    };
    let location = format!("{}/{}", text("warehouse_code"), text("location_code"));
    let input = crate::notifications::emitter::NotificationEventInput {
        source_module: "inventory_count_lines".into(),
        source_record_id: Some(pending.line_id.to_string()),
        event_code: "stock_count_stale".into(),
        category_code: RECOUNT_CATEGORY.into(),
        severity: "warning".into(),
        // Named by the line's sync id (the same on every PC), never its local id.
        dedupe_key: Some(format!("stock-count-recount|{line_sync_id}")),
        payload_json: None,
        title: format!("Recomptage demandé — {code} à {location}"),
        body: Some(format!(
            "{name} : le stock de {location} a bougé depuis le comptage {} (de {} à {on_hand}) ; \
             son approbation a été refusée. Recomptez l'emplacement.",
            pending.session_code, pending.system_qty
        )),
        action_url: Some(LIST_URL.to_string()),
    };
    let message = crate::notifications::emitter::NotificationMessage::new(
        RECOUNT_CATEGORY,
        serde_json::json!({
            "code": code,
            "title": name,
            "location": location,
            "session": pending.session_code,
            "system_qty": pending.system_qty,
            "on_hand": on_hand,
        }),
    );
    if let Err(err) = crate::notifications::emitter::emit_event_with(db, input, Some(&message)).await {
        tracing::warn!(error = %err, line_id = pending.line_id, "recount notice not raised");
    }
}
