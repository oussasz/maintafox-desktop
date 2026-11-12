//! Hors-stock articles bought for one work order (L3-04, CMMS Advisor option B).
//!
//! A hors-stock article is an article whose stocking type (system reference list
//! `inventory.stocking_type`) is `NON_STOCK`. It never enters the store:
//! - the work-order editor asks for it on the work order: an existing hors-stock article is
//!   reused, otherwise one is created from the work order, marked "à revoir" for the catalogue
//!   manager (`create_wo_non_stock_purchase`);
//! - the requisition points at that work-order line and goes through the normal approvals and
//!   ceilings; the buyer orders it;
//! - the receipt makes no stock movement and no valuation: the accepted quantity and its receipt
//!   cost are booked on the work-order line, which work-order costing counts as parts actual
//!   (`book_receipt_to_work_order`);
//! - the work order does not close while such a purchase is still open (`wo::closeout`).
//!
//! A hors-stock article never has a stock balance: manual entries, counts, reservations and
//! transfers refuse it (`ensure_store_article`), so stock lists, counts and valuation never show
//! it; reorder and the stock-out indicator exclude it explicitly. The catalogue manager may
//! promote one to a stocked article or mark it a duplicate of another
//! (`review_non_stock_article`); nothing is ever deleted.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{CreateProcurementRequisitionInput, ProcurementRequisition};

/// Code of the hors-stock value in the system list `inventory.stocking_type`.
pub const NON_STOCK_CODE: &str = "NON_STOCK";
const STOCKED_CODE: &str = "STOCKED";
const STOCKING_TYPE_DOMAIN: &str = "inventory.stocking_type";
/// Demand source of a requisition raised for a work-order part line.
pub const WO_PART_DEMAND: &str = "WORK_ORDER_PART";

pub const REVIEW_TO_REVIEW: &str = "TO_REVIEW";
pub const REVIEW_REVIEWED: &str = "REVIEWED";
pub const REVIEW_DUPLICATE: &str = "DUPLICATE";

/// Stable refusal codes (translated in `errors`).
pub mod codes {
    pub const NEEDS_WORK_ORDER: &str = "GATE_INV_NON_STOCK_NEEDS_WO";
    pub const NO_WORK_ORDER_LINE: &str = "GATE_INV_NON_STOCK_NO_WO_LINE";
    pub const NOT_NON_STOCK: &str = "GATE_INV_NON_STOCK_NOT_HORS_STOCK";
    pub const TYPE_LOCKED: &str = "GATE_INV_NON_STOCK_TYPE_LOCKED";
    pub const DUPLICATE: &str = "GATE_INV_NON_STOCK_DUPLICATE";
    pub const NAME_REQUIRED: &str = "GATE_INV_NON_STOCK_NAME_REQUIRED";
    pub const UNIT_REQUIRED: &str = "GATE_INV_NON_STOCK_UNIT_REQUIRED";
    pub const TYPE_MISSING: &str = "GATE_INV_NON_STOCK_TYPE_MISSING";
    pub const REVIEW_ACTION: &str = "GATE_INV_NON_STOCK_REVIEW_ACTION";
    pub const NOT_IN_STORE: &str = "GATE_INV_NON_STOCK_NOT_IN_STORE";
    pub const PROMOTE_OPEN_PURCHASE: &str = "GATE_INV_NON_STOCK_PROMOTE_OPEN_PURCHASE";
    pub const PERSON_UNKNOWN: &str = "GATE_INV_NON_STOCK_PERSON_UNKNOWN";
}

fn refusal(code: &str, message: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {message}")])
}

pub async fn is_non_stock_article<C: ConnectionTrait>(conn: &C, article_id: i64) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS hit FROM articles a JOIN lookup_values lv ON lv.id = a.stocking_type_value_id \
             WHERE a.id = ? AND lv.code = ?",
            [article_id.into(), NON_STOCK_CODE.into()],
        ))
        .await?;
    Ok(row.is_some())
}

async fn stocking_type_value_id<C: ConnectionTrait>(conn: &C, code: &str) -> AppResult<i64> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = ? AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [STOCKING_TYPE_DOMAIN.into(), code.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(r.try_get("", "id")?),
        None => Err(refusal(
            codes::TYPE_MISSING,
            &format!("The stocking type '{code}' is missing or inactive in the reference data."),
        )),
    }
}

/// A requisition for a hors-stock article points at a work-order line (raised from the work
/// order); any other requisition is for a stock article.
pub async fn ensure_requisition_demand<C: ConnectionTrait>(
    conn: &C,
    input: &CreateProcurementRequisitionInput,
) -> AppResult<()> {
    if !is_non_stock_article(conn, input.article_id).await? {
        return Ok(());
    }
    let (Some(wo_id), Some(line_id)) = (input.demand_source_id, input.demand_source_line_id) else {
        return Err(refusal(
            codes::NEEDS_WORK_ORDER,
            "A hors-stock article is bought for a work order: ask for it from the work order.",
        ));
    };
    if input.demand_source_type != WO_PART_DEMAND {
        return Err(refusal(
            codes::NEEDS_WORK_ORDER,
            "A hors-stock article is bought for a work order: ask for it from the work order.",
        ));
    }
    // The line must be a live line of that work order for this very article: the receipt books
    // onto it.
    let line = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS hit FROM work_order_parts \
             WHERE id = ? AND work_order_id = ? AND article_id = ? AND consumption_status <> 'cancelled'",
            [line_id.into(), wo_id.into(), input.article_id.into()],
        ))
        .await?;
    if line.is_none() {
        return Err(refusal(
            codes::NO_WORK_ORDER_LINE,
            "The work-order line of this hors-stock purchase does not match the work order or the article.",
        ));
    }
    Ok(())
}

/// A hors-stock article never enters the store: no manual entry, count, reservation or transfer.
pub async fn ensure_store_article<C: ConnectionTrait>(conn: &C, article_id: i64) -> AppResult<()> {
    if is_non_stock_article(conn, article_id).await? {
        return Err(refusal(
            codes::NOT_IN_STORE,
            "A hors-stock article never enters the store: it is received on its work order.",
        ));
    }
    Ok(())
}

/// A multi-line (stock) requisition takes no hors-stock article.
pub async fn ensure_stock_article_line<C: ConnectionTrait>(conn: &C, article_id: i64) -> AppResult<()> {
    if is_non_stock_article(conn, article_id).await? {
        return Err(refusal(
            codes::NEEDS_WORK_ORDER,
            "A hors-stock article is bought for a work order: ask for it from the work order.",
        ));
    }
    Ok(())
}

/// Condition on `wop` (a work-order part line): a purchase raised for it is still open — an
/// active requisition not yet ordered, or an order line still to receive.
const OPEN_PURCHASE_SQL: &str = "( \
    EXISTS (SELECT 1 FROM purchase_order_lines pol \
             JOIN purchase_orders po ON po.id = pol.purchase_order_id \
            WHERE pol.demand_source_type = 'WORK_ORDER_PART' AND pol.demand_source_line_id = wop.id \
              AND pol.status = 'OPEN' AND po.status NOT IN ('CANCELLED', 'RECEIVED_CLOSED')) \
 OR EXISTS (SELECT 1 FROM procurement_requisition_lines prl \
             JOIN procurement_requisitions pr ON pr.id = prl.requisition_id \
            WHERE pr.demand_source_type = 'WORK_ORDER_PART' AND prl.demand_source_line_id = wop.id \
              AND pr.status IN ('DRAFT', 'SUBMITTED', 'APPROVED') \
              AND NOT EXISTS (SELECT 1 FROM purchase_order_lines pol2 \
                               JOIN purchase_orders po2 ON po2.id = pol2.purchase_order_id \
                              WHERE pol2.requisition_line_id = prl.id AND po2.status <> 'CANCELLED')))";

pub async fn part_has_open_purchase<C: ConnectionTrait>(conn: &C, wo_part_id: i64) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT 1 AS hit FROM work_order_parts wop WHERE wop.id = ? AND {OPEN_PURCHASE_SQL}"),
            [wo_part_id.into()],
        ))
        .await?;
    Ok(row.is_some())
}

/// Hors-stock lines of a work order whose purchase is still open (close gate).
pub async fn open_purchase_line_count<C: ConnectionTrait>(conn: &C, work_order_id: i64) -> AppResult<i64> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS n FROM work_order_parts wop \
                 WHERE wop.work_order_id = ? AND wop.consumption_status <> 'cancelled' AND {OPEN_PURCHASE_SQL}"
            ),
            [work_order_id.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "n")).transpose()?.unwrap_or(0))
}

/// Receipt of a hors-stock order line: no stock movement, the accepted quantity is booked on its
/// work-order line at the receipt cost (tenant currency), which costing counts as parts actual.
/// Without an agreed price the received quantity stays unvalued (no cost is invented).
pub async fn book_receipt_to_work_order<C: ConnectionTrait>(
    conn: &C,
    po_line_id: i64,
    accepted_qty: f64,
    unit_cost_tenant: Option<f64>,
) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wop.id, COALESCE(wop.non_stock_received_qty, 0) AS received, wop.unit_cost
             FROM purchase_order_lines pol
             JOIN work_order_parts wop ON wop.id = pol.demand_source_line_id
                                      AND wop.work_order_id = pol.demand_source_id
                                      AND wop.article_id = pol.article_id
             WHERE pol.id = ? AND pol.demand_source_type = 'WORK_ORDER_PART'",
            [po_line_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(refusal(
            codes::NO_WORK_ORDER_LINE,
            "This hors-stock order line is not linked to a work-order line: it cannot be received.",
        ));
    };
    if accepted_qty <= 0.0 {
        return Ok(());
    }
    let part_id: i64 = row.try_get("", "id")?;
    let received: f64 = row.try_get("", "received")?;
    let current_cost: Option<f64> = row.try_get("", "unit_cost")?;
    let total = received + accepted_qty;
    // Before the first receipt the line carries the estimate, not a cost: the receipt replaces it.
    // Later receipts average with the cost already booked; an unvalued part keeps the line unvalued.
    let unit_cost = match (unit_cost_tenant, received > 0.0) {
        (Some(cost), false) => Some(cost),
        (Some(cost), true) => current_cost.map(|c| (c * received + cost * accepted_qty) / total),
        (None, _) => None,
    };
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts
         SET non_stock_received_qty = ?, quantity_used = ?, unit_cost = ?, consumption_status = 'used'
         WHERE id = ?",
        [
            total.into(),
            total.into(),
            unit_cost.map_or(Value::Double(None), Value::from),
            part_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

// ── Work-order request ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonStockArticle {
    pub id: i64,
    pub article_code: String,
    pub article_name: String,
    pub unit_value_id: Option<i64>,
    pub unit_symbol: Option<String>,
    pub origin_work_order_id: Option<i64>,
    pub origin_work_order_code: Option<String>,
    pub catalogue_review_state: Option<String>,
    pub duplicate_of_article_id: Option<i64>,
    pub is_active: bool,
}

const ARTICLE_SELECT: &str = concat!(
    "SELECT a.id, a.article_code, a.article_name, a.unit_value_id, ",
    crate::reference::units::unit_symbol_of!("a.unit_value_id"),
    " AS unit_symbol, a.origin_work_order_id, wo.code AS origin_work_order_code, \
     a.catalogue_review_state, a.duplicate_of_article_id, a.is_active \
     FROM articles a \
     JOIN lookup_values lv ON lv.id = a.stocking_type_value_id AND lv.code = 'NON_STOCK' \
     LEFT JOIN work_orders wo ON wo.id = a.origin_work_order_id"
);

fn map_article(row: &sea_orm::QueryResult) -> AppResult<NonStockArticle> {
    Ok(NonStockArticle {
        id: row.try_get("", "id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        unit_value_id: row.try_get("", "unit_value_id")?,
        unit_symbol: row.try_get("", "unit_symbol")?,
        origin_work_order_id: row.try_get("", "origin_work_order_id")?,
        origin_work_order_code: row.try_get("", "origin_work_order_code")?,
        catalogue_review_state: row.try_get("", "catalogue_review_state")?,
        duplicate_of_article_id: row.try_get("", "duplicate_of_article_id")?,
        is_active: row.try_get::<i64>("", "is_active")? == 1,
    })
}

async fn get_article<C: ConnectionTrait>(conn: &C, article_id: i64) -> AppResult<NonStockArticle> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{ARTICLE_SELECT} WHERE a.id = ?"),
            [article_id.into()],
        ))
        .await?;
    match row {
        Some(r) => map_article(&r),
        None => Err(refusal(
            codes::NOT_NON_STOCK,
            "This article is not a hors-stock article.",
        )),
    }
}

/// Active hors-stock articles matching the text (code or name), to reuse one before creating a
/// new one. Duplicates are left out: the article they duplicate is offered instead.
pub async fn search_non_stock_articles(
    db: &DatabaseConnection,
    query: Option<String>,
) -> AppResult<Vec<NonStockArticle>> {
    let pattern = format!("%{}%", query.unwrap_or_default().trim());
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{ARTICLE_SELECT} WHERE a.is_active = 1 \
                 AND COALESCE(a.catalogue_review_state, '') <> 'DUPLICATE' \
                 AND (a.article_code LIKE ? OR a.article_name LIKE ?) \
                 ORDER BY a.article_name LIMIT 50"
            ),
            [pattern.clone().into(), pattern.into()],
        ))
        .await?;
    rows.iter().map(map_article).collect()
}

/// Hors-stock articles for the catalogue manager: those "à revoir" first, then the others.
pub async fn list_non_stock_articles(db: &DatabaseConnection, to_review_only: bool) -> AppResult<Vec<NonStockArticle>> {
    let filter = if to_review_only {
        " WHERE a.catalogue_review_state = 'TO_REVIEW'"
    } else {
        ""
    };
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "{ARTICLE_SELECT}{filter} \
                 ORDER BY CASE WHEN a.catalogue_review_state = 'TO_REVIEW' THEN 0 ELSE 1 END, a.article_name"
            ),
        ))
        .await?;
    rows.iter().map(map_article).collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateWoNonStockPurchaseInput {
    pub work_order_id: i64,
    /// Existing hors-stock article to reuse; otherwise `article_name` + `unit_value_id` create one.
    pub article_id: Option<i64>,
    pub article_name: Option<String>,
    pub unit_value_id: Option<i64>,
    pub quantity: f64,
    /// Estimated unit price (tenant currency): approval amount and starting price of the order.
    pub estimated_unit_cost: Option<f64>,
    pub notes: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WoNonStockPurchase {
    pub part: crate::wo::parts::WoPart,
    pub article: NonStockArticle,
    pub requisition: ProcurementRequisition,
}

/// Asks for a hors-stock part on a work order: reuses or creates the hors-stock article, writes
/// the work-order line and the requisition pointing at it, all in one transaction.
pub async fn create_wo_non_stock_purchase(
    db: &DatabaseConnection,
    input: CreateWoNonStockPurchaseInput,
    actor_id: i64,
) -> AppResult<WoNonStockPurchase> {
    if !input.quantity.is_finite() || input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_QTY_POSITIVE: The quantity must be greater than zero.".to_string(),
        ]));
    }
    if let Some(cost) = input.estimated_unit_cost {
        if !cost.is_finite() || cost < 0.0 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_INV_PRICE_NEGATIVE: An estimated unit price cannot be negative.".to_string(),
            ]));
        }
    }
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM work_orders WHERE id = ?",
            [input.work_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "work_orders".to_string(),
            id: input.work_order_id.to_string(),
        })?;
    let wo_code: String = wo_row.try_get("", "code")?;

    let tx = db.begin().await?;
    let article_id = match input.article_id {
        Some(id) => {
            let article = get_article(&tx, id).await?;
            if !article.is_active {
                return Err(AppError::ValidationFailed(vec![
                    "GATE_INV_ARTICLE_INACTIVE: This article is inactive.".to_string(),
                ]));
            }
            if article.catalogue_review_state.as_deref() == Some(REVIEW_DUPLICATE) {
                return Err(refusal(
                    codes::DUPLICATE,
                    "This article was marked a duplicate: choose the article it duplicates.",
                ));
            }
            id
        }
        None => {
            let name = input.article_name.as_deref().map(str::trim).unwrap_or_default();
            if name.is_empty() {
                return Err(refusal(codes::NAME_REQUIRED, "Describe the part to buy."));
            }
            let Some(unit_value_id) = input.unit_value_id else {
                return Err(refusal(codes::UNIT_REQUIRED, "Choose the unit of the part to buy."));
            };
            crate::reference::units::ensure_unit(&tx, unit_value_id, "unit_value_id", true).await?;
            create_article_from_work_order(&tx, name, unit_value_id, input.work_order_id, actor_id).await?
        }
    };

    let part = crate::wo::parts::insert_non_stock_line(
        &tx,
        input.work_order_id,
        article_id,
        input.quantity,
        input.estimated_unit_cost,
        input.notes.clone(),
    )
    .await?;
    let req_id = crate::inventory::procurement::insert_procurement_requisition(
        &tx,
        CreateProcurementRequisitionInput {
            article_id,
            preferred_location_id: None,
            requested_qty: input.quantity,
            demand_source_type: WO_PART_DEMAND.to_string(),
            demand_source_id: Some(input.work_order_id),
            demand_source_ref: Some(wo_code),
            demand_source_line_id: Some(part.id),
            source_reservation_id: None,
            source_reorder_trigger: None,
            purchase_priority: None,
            reason: input.reason.clone(),
            actor_id: Some(actor_id),
        },
        input.estimated_unit_cost,
    )
    .await?;
    let article = get_article(&tx, article_id).await?;
    tx.commit().await?;
    let requisition = crate::inventory::procurement::get_procurement_requisition(db, req_id).await?;
    Ok(WoNonStockPurchase {
        part,
        article,
        requisition,
    })
}

async fn create_article_from_work_order<C: ConnectionTrait>(
    conn: &C,
    name: &str,
    unit_value_id: i64,
    work_order_id: i64,
    actor_id: i64,
) -> AppResult<i64> {
    let stocking_type_id = stocking_type_value_id(conn, NON_STOCK_CODE).await?;
    // Per-device number: two PCs never draw the same code.
    let code = crate::sync::device_code::next_device_number(conn, "articles", "article_code", "HS", 4).await?;
    let inserted = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO articles (
                entity_sync_id, article_code, article_name, unit_value_id, stocking_type_value_id,
                min_stock, reorder_point, safety_stock, is_active, row_version,
                origin_work_order_id, catalogue_review_state
             ) VALUES (?, ?, ?, ?, ?, 0, 0, 0, 1, 1, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                code.clone().into(),
                name.to_string().into(),
                unit_value_id.into(),
                stocking_type_id.into(),
                work_order_id.into(),
                REVIEW_TO_REVIEW.into(),
            ],
        ))
        .await?;
    let article_id = crate::db::inserted_id(&inserted)?;
    crate::inventory::procurement::record_state_event(
        conn,
        "ARTICLE",
        article_id,
        None,
        "CREATED",
        Some(actor_id),
        Some("article.created_from_work_order"),
        Some(&format!("Hors-stock article {code} created from a work order")),
    )
    .await?;
    Ok(article_id)
}

// ── Catalogue review ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewNonStockArticleInput {
    pub article_id: i64,
    /// `KEEP` (stays hors stock, reviewed) | `PROMOTE` (becomes a stocked article) |
    /// `DUPLICATE` (duplicate of `duplicate_of_article_id`).
    pub action: String,
    pub duplicate_of_article_id: Option<i64>,
}

/// The catalogue manager's review of a hors-stock article. Nothing is deleted: a duplicate is
/// only marked so, and is no longer offered for new requests.
pub async fn review_non_stock_article(
    db: &DatabaseConnection,
    input: ReviewNonStockArticleInput,
    actor_id: i64,
) -> AppResult<NonStockArticle> {
    let tx = db.begin().await?;
    let article = get_article(&tx, input.article_id).await?;
    let now = crate::inventory::procurement::now_iso();
    let (sql, values, to_state): (&str, Vec<Value>, &str) = match input.action.as_str() {
        "KEEP" => (
            "UPDATE articles SET catalogue_review_state = ?, duplicate_of_article_id = NULL, \
             row_version = row_version + 1, updated_at = ? WHERE id = ?",
            vec![REVIEW_REVIEWED.into(), now.into(), article.id.into()],
            REVIEW_REVIEWED,
        ),
        "PROMOTE" => {
            // A delivery still expected for a work order would otherwise go into the store
            // instead of onto that work order.
            let open = tx
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT 1 AS hit FROM work_order_parts wop \
                         WHERE wop.article_id = ? AND wop.consumption_status <> 'cancelled' AND {OPEN_PURCHASE_SQL}"
                    ),
                    [article.id.into()],
                ))
                .await?;
            if open.is_some() {
                return Err(refusal(
                    codes::PROMOTE_OPEN_PURCHASE,
                    "A work-order purchase of this article is still open: receive or cancel it before moving the article to stock.",
                ));
            }
            let stocked = stocking_type_value_id(&tx, STOCKED_CODE).await?;
            (
                "UPDATE articles SET stocking_type_value_id = ?, catalogue_review_state = ?, \
                 duplicate_of_article_id = NULL, row_version = row_version + 1, updated_at = ? WHERE id = ?",
                vec![stocked.into(), REVIEW_REVIEWED.into(), now.into(), article.id.into()],
                "PROMOTED",
            )
        }
        "DUPLICATE" => {
            let Some(target) = input.duplicate_of_article_id.filter(|id| *id != article.id) else {
                return Err(refusal(
                    codes::REVIEW_ACTION,
                    "Choose the article this one duplicates (another article).",
                ));
            };
            let exists = tx
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS hit FROM articles WHERE id = ? AND is_active = 1 \
                     AND COALESCE(catalogue_review_state, '') <> 'DUPLICATE'",
                    [target.into()],
                ))
                .await?;
            if exists.is_none() {
                return Err(refusal(
                    codes::REVIEW_ACTION,
                    "The article it duplicates must exist, be active and not be a duplicate itself.",
                ));
            }
            (
                "UPDATE articles SET catalogue_review_state = ?, duplicate_of_article_id = ?, \
                 row_version = row_version + 1, updated_at = ? WHERE id = ?",
                vec![REVIEW_DUPLICATE.into(), target.into(), now.into(), article.id.into()],
                REVIEW_DUPLICATE,
            )
        }
        _ => {
            return Err(refusal(
                codes::REVIEW_ACTION,
                "Unknown review action (KEEP, PROMOTE or DUPLICATE).",
            ))
        }
    };
    tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    crate::inventory::procurement::record_state_event(
        &tx,
        "ARTICLE",
        article.id,
        article.catalogue_review_state.as_deref(),
        to_state,
        Some(actor_id),
        Some("article.non_stock_reviewed"),
        None,
    )
    .await?;
    let reviewed = if to_state == "PROMOTED" {
        // No longer a hors-stock article: the hors-stock view of it is returned as reviewed.
        NonStockArticle {
            catalogue_review_state: Some(REVIEW_REVIEWED.to_string()),
            duplicate_of_article_id: None,
            ..article
        }
    } else {
        get_article(&tx, article.id).await?
    };
    tx.commit().await?;
    Ok(reviewed)
}

/// "Remis à <personne>" on a received hors-stock line (or cleared with `None`).
pub async fn set_part_handed_to(
    db: &DatabaseConnection,
    wo_part_id: i64,
    personnel_id: Option<i64>,
) -> AppResult<crate::wo::parts::WoPart> {
    let part = crate::wo::parts::load_part_in(db, wo_part_id).await?;
    if !part.is_non_stock || part.non_stock_received_qty <= 0.0 {
        return Err(refusal(
            codes::NOT_NON_STOCK,
            "Only a received hors-stock part is handed to someone.",
        ));
    }
    if let Some(person) = personnel_id {
        let known = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS hit FROM personnel WHERE id = ?",
                [person.into()],
            ))
            .await?;
        if known.is_none() {
            return Err(refusal(
                codes::PERSON_UNKNOWN,
                "The person it was handed to is unknown.",
            ));
        }
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts SET handed_to_personnel_id = ? WHERE id = ?",
        [personnel_id.map_or(Value::BigInt(None), Value::from), wo_part_id.into()],
    ))
    .await?;
    crate::wo::parts::load_part_in(db, wo_part_id).await
}
