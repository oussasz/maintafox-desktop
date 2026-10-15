//! One issue path for stock leaving the store for a job.
//!
//! The issue desk (storekeeper scans a part for a work order) and the WO module (parts used on
//! an OT, through `queries::issue_reserved_stock`) both post through [`issue_stock`]:
//! - an ISSUE always consumes a reservation (`ledger::movement_balance_effect`); without one, the
//!   desk reserves for the work order first, in the same transaction;
//! - tracked articles issue from the chosen lot/serial, or FEFO across lots (one ISSUE per lot);
//! - each ISSUE is valued at the location's weighted-average cost;
//! - postings in a closed valuation period are refused (ledger insert guard).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{InventoryReserveInput, IssueOutcome, IssueStockRequest, IssuedLine, ScanResolution};
use crate::inventory::lots;
use crate::inventory::queries;

/// Work-order statuses in which parts can be issued to the job (same list as the desk picker).
pub const PARTS_OPEN_WO_STATUSES: &[&str] = &["planning", "ready", "in_progress", "on_hold"];

/// Movement source written on the ISSUE when the caller has its own (WO module).
#[derive(Debug, Clone, Default)]
pub struct IssueSourceOverride {
    pub source_type: Option<String>,
    pub source_id: Option<i64>,
    pub source_ref: Option<String>,
}

pub async fn issue_stock(db: &DatabaseConnection, request: IssueStockRequest) -> AppResult<IssueOutcome> {
    issue_stock_with_source(db, request, IssueSourceOverride::default()).await
}

async fn work_order_code<C: ConnectionTrait>(db: &C, work_order_id: i64) -> AppResult<(String, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.code, COALESCE(s.code, '') AS status_code
             FROM work_orders wo LEFT JOIN work_order_statuses s ON s.id = wo.status_id
             WHERE wo.id = ?",
            [work_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "work_orders".to_string(),
            id: work_order_id.to_string(),
        })?;
    Ok((row.try_get("", "code")?, row.try_get("", "status_code")?))
}

pub async fn issue_stock_with_source(
    db: &DatabaseConnection,
    request: IssueStockRequest,
    source: IssueSourceOverride,
) -> AppResult<IssueOutcome> {
    if !request.quantity.is_finite() || request.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "The issued quantity must be greater than 0.".to_string(),
        ]));
    }
    let tracking = lots::article_tracking(db, request.article_id).await?;
    lots::ensure_whole_units(&tracking, request.quantity)?;

    let tx = db.begin().await?;
    let (reservation_id, work_order_id, work_order_code_value) = match request.reservation_id {
        Some(reservation_id) => {
            let row = tx
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT r.article_id, r.location_id, r.source_type, r.source_id,
                            CASE WHEN r.source_type IN ('WO', 'WORK_ORDER', 'WORK_ORDER_PART', 'work_order')
                                 THEN wo.code END AS wo_code
                     FROM stock_reservations r
                     LEFT JOIN work_orders wo ON wo.id = r.source_id
                         AND r.source_type IN ('WO', 'WORK_ORDER', 'WORK_ORDER_PART', 'work_order')
                     WHERE r.id = ?",
                    [reservation_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "stock_reservations".to_string(),
                    id: reservation_id.to_string(),
                })?;
            let article_id: i64 = row.try_get("", "article_id")?;
            let location_id: i64 = row.try_get("", "location_id")?;
            if article_id != request.article_id || location_id != request.location_id {
                return Err(AppError::ValidationFailed(vec![
                    "The reservation is for another article or location.".to_string(),
                ]));
            }
            let wo_code: Option<String> = row.try_get("", "wo_code").ok().flatten();
            let wo_id: Option<i64> = if wo_code.is_some() {
                row.try_get("", "source_id").ok().flatten()
            } else {
                None
            };
            (reservation_id, wo_id, wo_code)
        }
        None => {
            let Some(work_order_id) = request.work_order_id else {
                return Err(AppError::ValidationFailed(vec![
                    "An issue from the store names the work order the parts go to.".to_string(),
                ]));
            };
            let (code, status) = work_order_code(&tx, work_order_id).await?;
            if !PARTS_OPEN_WO_STATUSES.contains(&status.as_str()) {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Work order {code} is not open for parts (status: {status})."
                )]));
            }
            let reservation_id = queries::reserve_stock_in(
                &tx,
                &InventoryReserveInput {
                    article_id: request.article_id,
                    location_id: request.location_id,
                    quantity: request.quantity,
                    source_type: "WORK_ORDER".to_string(),
                    source_id: Some(work_order_id),
                    source_ref: Some(code.clone()),
                    notes: request.notes.clone(),
                },
            )
            .await?;
            (reservation_id, Some(work_order_id), Some(code))
        }
    };

    let allocation: Vec<(Option<i64>, f64)> = if tracking.is_tracked() {
        match request.lot_id {
            Some(lot) => {
                lots::ensure_lot_available(&tx, lot, request.article_id, request.location_id, request.quantity).await?;
                vec![(Some(lot), request.quantity)]
            }
            None => lots::fefo_allocate(&tx, request.article_id, request.location_id, request.quantity)
                .await?
                .into_iter()
                .map(|(lot, qty)| (Some(lot), qty))
                .collect(),
        }
    } else {
        if request.lot_id.is_some() {
            return Err(AppError::ValidationFailed(vec![
                "This article is not lot-tracked; issue it without a lot.".to_string(),
            ]));
        }
        vec![(None, request.quantity)]
    };

    let source_type = source.source_type.clone().or_else(|| Some("WORK_ORDER".to_string()));
    let source_id = source.source_id.or(work_order_id);
    let source_ref = source.source_ref.clone().or_else(|| work_order_code_value.clone());
    let mut lines = Vec::new();
    for (lot_id, quantity) in allocation {
        let (transaction_id, unit_cost) = queries::issue_reserved_in(
            &tx,
            reservation_id,
            quantity,
            lot_id,
            // Reservations of the WO module keep their own source type.
            if request.reservation_id.is_some() {
                source.source_type.as_deref()
            } else {
                source_type.as_deref()
            },
            source_id,
            source_ref.as_deref(),
            request.notes.as_deref(),
            request.actor_id,
        )
        .await?;
        let (lot_number, serial_number) = match lot_id {
            Some(lot) => {
                let row = tx
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT lot_number, serial_number FROM inventory_lots WHERE id = ?",
                        [lot.into()],
                    ))
                    .await?;
                match row {
                    Some(r) => (
                        r.try_get("", "lot_number").ok().flatten(),
                        r.try_get("", "serial_number").ok().flatten(),
                    ),
                    None => (None, None),
                }
            }
            None => (None, None),
        };
        lines.push(IssuedLine {
            transaction_id,
            lot_id,
            lot_number,
            serial_number,
            quantity,
            unit_cost,
            extended_cost: unit_cost.map(|c| c * quantity),
        });
    }
    tx.commit().await?;

    let valued: Vec<f64> = lines.iter().filter_map(|l| l.extended_cost).collect();
    Ok(IssueOutcome {
        reservation_id,
        work_order_id,
        work_order_code: work_order_code_value,
        total_cost: (!valued.is_empty()).then(|| valued.iter().sum()),
        lines,
    })
}

/// Resolves what a scanner (keyboard wedge) or a typed value names: an article barcode, an
/// article code, or a lot / serial number.
pub async fn resolve_scan(db: &DatabaseConnection, value: String) -> AppResult<ScanResolution> {
    let scanned = value.trim().to_string();
    if scanned.is_empty() {
        return Err(AppError::ValidationFailed(vec!["Nothing was scanned.".to_string()]));
    }
    let article = |row: sea_orm::QueryResult, lot_id: Option<i64>, matched: &str| -> AppResult<ScanResolution> {
        Ok(ScanResolution {
            article_id: row.try_get("", "id")?,
            article_code: row.try_get("", "article_code")?,
            article_name: row.try_get("", "article_name")?,
            unit_code: row.try_get("", "unit_code")?,
            lot_id,
            matched_on: matched.to_string(),
        })
    };
    const ARTICLE_SELECT: &str = "SELECT a.id, a.article_code, a.article_name, u.code AS unit_code
         FROM articles a JOIN reference_values u ON u.id = a.unit_value_id";
    for (column, matched) in [("a.barcode", "barcode"), ("a.article_code", "article_code")] {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("{ARTICLE_SELECT} WHERE a.is_active = 1 AND UPPER({column}) = UPPER(?)"),
                [scanned.clone().into()],
            ))
            .await?;
        if rows.len() > 1 {
            return Err(AppError::ValidationFailed(vec![format!(
                "'{scanned}' matches several articles; scan the article code instead."
            )]));
        }
        if let Some(row) = rows.into_iter().next() {
            return article(row, None, matched);
        }
    }
    let lots = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT l.id AS lot_id, l.article_id FROM inventory_lots l
             JOIN articles a ON a.id = l.article_id AND a.is_active = 1
             WHERE UPPER(l.serial_number) = UPPER(?) OR UPPER(l.lot_number) = UPPER(?)",
            [scanned.clone().into(), scanned.clone().into()],
        ))
        .await?;
    if lots.len() == 1 {
        let lot_row = lots.into_iter().next().expect("one lot");
        let lot_id: i64 = lot_row.try_get("", "lot_id")?;
        let article_id: i64 = lot_row.try_get("", "article_id")?;
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("{ARTICLE_SELECT} WHERE a.id = ?"),
                [article_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "articles".to_string(),
                id: article_id.to_string(),
            })?;
        return article(row, Some(lot_id), "lot");
    }
    if lots.len() > 1 {
        return Err(AppError::ValidationFailed(vec![format!(
            "'{scanned}' is a lot number used by several articles; scan the article first."
        )]));
    }
    Err(AppError::ValidationFailed(vec![format!(
        "No active article, barcode, lot or serial number matches '{scanned}'."
    )]))
}
