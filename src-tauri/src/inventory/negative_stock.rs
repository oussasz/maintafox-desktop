//! Stock balances below zero after sync: "négatif — à rapprocher" (CMMS Advisor 2026-10-20).
//!
//! A stock movement received from another PC is applied as received: it is a physical fact and the
//! ledger must be the same on every PC. Two PCs that each issued the last units of a location
//! therefore leave it below zero on both. Such a balance is not refused and not a sync conflict:
//! - on-hand stock below zero: one review item per article and location (`stock_negative_reviews`,
//!   at most one open per pair), whatever movement brought it there;
//! - on-hand stock at zero or more but more reserved than held: a warning only ("réservations
//!   supérieures au stock"), derived from the balance, never stored and never a review item.
//!
//! An item is open exactly while the location's on-hand stock is below zero: [`reconcile_pair_in`]
//! opens or closes it from the balance, after each received movement (`sync::apply_hooks`), after
//! the count that resolves it, and for every balance before the list is read (local movements made
//! by the inventory commands are caught there). Balances are derived from the synchronised ledger,
//! so every PC holding the same movements shows the same open items, whatever order they arrived
//! in. The items are this PC's projection and are not synchronised (`sync::registry`, derived
//! projection); what resolves them — the count session, its movement and its audit — is.
//!
//! The one resolution is a stock count of that article at that location
//! ([`resolve_by_count`], `inv.manage`, plant-wide while stores have no site), with a reason of at
//! least [`REVIEW_REASON_MIN_CHARS`] characters: it is posted through the count workflow
//! (`controls::record_location_count_in`) and audited. A later stock entry (goods receipt, return,
//! transfer in, manual entry) that brings the stock back to zero or more closes the item too,
//! noted "régularisé par réception". There is no reject and no "accept anyway".

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait, Value};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Shortest reason accepted for the count that resolves an item (characters, trimmed).
pub const REVIEW_REASON_MIN_CHARS: usize = 5;

/// Quantities closer to zero than this are zero (balances are sums of decimal movements).
const QTY_TOLERANCE: f64 = 1e-9;

/// Closed by a stock count (or the reversal of one).
pub const CLOSURE_COUNT: &str = "count";
/// Closed by a stock entry: "régularisé par réception".
pub const CLOSURE_RECEIPT: &str = "receipt";
/// Back to zero or more without a stock entry on record since the item opened (a data repair).
pub const CLOSURE_OTHER: &str = "other";

/// Movement types whose entry closes an item as a count.
const COUNT_MOVEMENTS: &[&str] = &["COUNT_ADJUST", "COUNT_REVERSAL"];

pub const REFUSED_REASON_SHORT: &str = "NEGATIVE_STOCK_REASON_SHORT";
pub const REFUSED_COUNT_INVALID: &str = "NEGATIVE_STOCK_COUNT_INVALID";
pub const REFUSED_VARIANCE_REASON_REQUIRED: &str = "NEGATIVE_STOCK_VARIANCE_REASON_REQUIRED";
pub const REFUSED_NOT_OPEN: &str = "NEGATIVE_STOCK_NOT_OPEN";

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
    /// `count`, `receipt` ("régularisé par réception") or `other`.
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NegativeStockReviewList {
    /// Open items first, then the closed ones when asked for.
    pub items: Vec<NegativeStockReview>,
    pub over_reservations: Vec<OverReservationWarning>,
}

/// Counts for the sync centre, derived from the balances.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegativeStockSummary {
    pub negative_count: i64,
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

fn refused(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn sqlite(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

/// Opens or closes the item of one article and location from its balance: open while the on-hand
/// stock is below zero (one open item per pair), closed as soon as it is zero or more, noted with
/// the latest stock entry since it opened (a count, or "régularisé par réception").
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
            conn.execute(sqlite(
                "INSERT INTO stock_negative_reviews
                    (article_id, location_id, status, opened_at, opening_transaction_id)
                 VALUES (?, ?, 'open', ?, ?)",
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
                        "SELECT t.id, t.movement_type, t.performed_at FROM inventory_transactions t
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
                    let kind = if COUNT_MOVEMENTS.contains(&movement_type.as_str()) {
                        CLOSURE_COUNT
                    } else {
                        CLOSURE_RECEIPT
                    };
                    // The time of the movement that closed it, never before the item opened here
                    // (a movement made earlier on another PC may arrive after the item opened).
                    let closed_at = performed_at.filter(|at| *at > opened_at).unwrap_or(opened_at);
                    (kind, Some(row.try_get::<i64>("", "id")?), closed_at)
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

/// The items to reconcile (and the closed ones when `include_closed`), and the over-reservation
/// warnings. The items are first brought in line with the balances.
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
        over_reservations,
    })
}

/// How many locations are below zero and how many hold more reserved than on hand (sync centre).
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
        over_reserved_count: row.try_get("", "over_reserved_count")?,
    })
}

/// Resolves the item of an article and location by a stock count recorded there by `user_id`:
/// needs `inv.manage` (plant-wide: stores have no site yet, as for stock adjustments), a reason of
/// at least [`REVIEW_REASON_MIN_CHARS`] characters, the quantity counted (zero or more) and a code
/// of the variance-reason list. The count is posted through the count workflow, audited, and the
/// item closes since the stock is now the counted quantity.
pub async fn resolve_by_count(
    db: &DatabaseConnection,
    user_id: i32,
    input: ResolveNegativeStockInput,
) -> AppResult<NegativeStockReview> {
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
    let item = tx
        .query_one(sqlite(
            "SELECT id FROM stock_negative_reviews WHERE article_id = ? AND location_id = ? AND status = 'open'",
            vec![input.article_id.into(), input.location_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            refused(
                REFUSED_NOT_OPEN,
                "This location's stock is no longer below zero: nothing to reconcile.",
                &[],
            )
        })?;
    let item_id: i64 = item.try_get("", "id")?;
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
        },
    )
    .await?;
    reconcile_pair_in(&tx, input.article_id, input.location_id).await?;
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "inventory.negative_stock.counted".to_string(),
            target_type: Some("stock_negative_review".to_string()),
            target_id: Some(item_id.to_string()),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Stock below zero reconciled by a count",
                "article_id": input.article_id,
                "location_id": input.location_id,
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
    load_review(db, item_id).await
}
