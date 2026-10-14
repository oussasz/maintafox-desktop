//! Lots, serial numbers and expiry dates of tracked articles.
//!
//! An article is tracked when `requires_batch_tracking` (lot number) or `requires_serial_tracking`
//! (one lot row per serial number, whole units) is set; `requires_expiration` adds a mandatory
//! expiry date. A lot's stock at a location is the sum of the ledger movements carrying its
//! `lot_id` (on-hand effect from `ledger::ON_HAND_MOVEMENTS`), so lots never drift from the ledger
//! and need no projection on devices that receive movements by sync.
//!
//! Picking is FEFO (first expiry, first out): lots expiring first, then lots without expiry, oldest
//! first. Expired lots are never picked automatically.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{ArticleTracking, LotStock, NewLotInput};
use crate::inventory::ledger::on_hand_delta_sql;

const EPS: f64 = 1e-9;

pub async fn article_tracking<C: ConnectionTrait>(db: &C, article_id: i64) -> AppResult<ArticleTracking> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(requires_batch_tracking, 0) AS batch,
                    COALESCE(requires_serial_tracking, 0) AS serial,
                    COALESCE(requires_expiration, 0) AS expiry
             FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "articles".to_string(),
            id: article_id.to_string(),
        })?;
    Ok(ArticleTracking {
        batch: row.try_get::<i64>("", "batch")? == 1,
        serial: row.try_get::<i64>("", "serial")? == 1,
        expiry: row.try_get::<i64>("", "expiry")? == 1,
    })
}

fn clean(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Creates (or, for a batch received again under the same number, reuses) the lot of a tracked
/// article and returns its id. Validates the article's tracking rules.
pub async fn create_or_reuse_lot<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    input: &NewLotInput,
    goods_receipt_line_id: Option<i64>,
    actor_id: Option<i64>,
) -> AppResult<i64> {
    let tracking = article_tracking(db, article_id).await?;
    if !tracking.is_tracked() {
        return Err(AppError::ValidationFailed(vec![
            "This article is not lot- or serial-tracked; no lot can be recorded.".to_string(),
        ]));
    }
    let lot_number = clean(input.lot_number.as_deref());
    let serial_number = clean(input.serial_number.as_deref());
    let expiry_date = clean(input.expiry_date.as_deref());
    if let Some(expiry) = expiry_date.as_deref() {
        crate::inventory::fx::parse_iso_date(expiry, "expiry_date")?;
    }
    if tracking.serial && serial_number.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "This article is serial-tracked: enter the serial number.".to_string(),
        ]));
    }
    if tracking.batch && !tracking.serial && lot_number.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "This article is lot-tracked: enter the lot number.".to_string(),
        ]));
    }
    if tracking.expiry && expiry_date.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "This article has a shelf life: enter the expiry date.".to_string(),
        ]));
    }

    if let Some(serial) = serial_number.as_deref() {
        let existing = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM inventory_lots WHERE article_id = ? AND serial_number = ?",
                [article_id.into(), serial.into()],
            ))
            .await?;
        if let Some(row) = existing {
            let lot_id: i64 = row.try_get("", "id")?;
            // A serial that left the store (issued, repaired…) may come back: reuse it only when
            // no unit of it is in stock anywhere.
            if lot_total_on_hand(db, lot_id).await? > EPS {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Serial number {serial} is already in stock."
                )]));
            }
            return Ok(lot_id);
        }
    } else if let Some(lot) = lot_number.as_deref() {
        let existing = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, expiry_date FROM inventory_lots
                 WHERE article_id = ? AND lot_number = ? AND serial_number IS NULL",
                [article_id.into(), lot.into()],
            ))
            .await?;
        if let Some(row) = existing {
            let lot_id: i64 = row.try_get("", "id")?;
            let stored: Option<String> = row.try_get("", "expiry_date")?;
            if stored != expiry_date {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Lot {lot} is already recorded with expiry {}; a lot has one expiry date.",
                    stored.unwrap_or_else(|| "—".to_string())
                )]));
            }
            return Ok(lot_id);
        }
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_lots
            (entity_sync_id, article_id, lot_number, serial_number, expiry_date, goods_receipt_line_id,
             notes, row_version, created_by_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, NULL, 1, ?, ?, ?)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            article_id.into(),
            lot_number.map_or(Value::String(None), Value::from),
            serial_number.map_or(Value::String(None), Value::from),
            expiry_date.map_or(Value::String(None), Value::from),
            goods_receipt_line_id.map_or(Value::BigInt(None), Value::from),
            actor_id.map_or(Value::BigInt(None), Value::from),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read lot id")))?;
    Ok(row.try_get("", "id")?)
}

async fn lot_total_on_hand<C: ConnectionTrait>(db: &C, lot_id: i64) -> AppResult<f64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(SUM({}), 0) AS qty FROM inventory_transactions t WHERE t.lot_id = ?",
                on_hand_delta_sql("t")
            ),
            [lot_id.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<f64>("", "qty")).transpose()?.unwrap_or(0.0))
}

/// Stock of one lot at one location.
pub async fn lot_stock_at<C: ConnectionTrait>(db: &C, lot_id: i64, location_id: i64) -> AppResult<f64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(SUM({}), 0) AS qty FROM inventory_transactions t
                 WHERE t.lot_id = ? AND t.location_id = ?",
                on_hand_delta_sql("t")
            ),
            [lot_id.into(), location_id.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<f64>("", "qty")).transpose()?.unwrap_or(0.0))
}

fn lot_stock_sql(filters: &str) -> String {
    format!(
        "SELECT l.id AS lot_id, l.article_id, a.article_code, a.article_name, l.lot_number, l.serial_number,
                l.expiry_date, t.location_id, sl.code AS location_code, sl.warehouse_id, w.code AS warehouse_code,
                SUM({delta}) AS quantity,
                CASE WHEN l.expiry_date IS NULL THEN NULL
                     ELSE CAST(julianday(l.expiry_date) - julianday(date('now')) AS INTEGER) END AS days_to_expiry
         FROM inventory_lots l
         JOIN inventory_transactions t ON t.lot_id = l.id
         JOIN articles a ON a.id = l.article_id
         JOIN stock_locations sl ON sl.id = t.location_id
         JOIN warehouses w ON w.id = sl.warehouse_id
         WHERE 1 = 1 {filters}
         GROUP BY l.id, t.location_id
         HAVING SUM({delta}) > 0.000000001
         ORDER BY CASE WHEN l.expiry_date IS NULL THEN 1 ELSE 0 END, l.expiry_date, l.created_at, l.id",
        delta = on_hand_delta_sql("t"),
    )
}

fn map_lot_stock(row: sea_orm::QueryResult) -> AppResult<LotStock> {
    Ok(LotStock {
        lot_id: row.try_get("", "lot_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        lot_number: row.try_get("", "lot_number")?,
        serial_number: row.try_get("", "serial_number")?,
        expiry_date: row.try_get("", "expiry_date")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        warehouse_id: row.try_get("", "warehouse_id")?,
        warehouse_code: row.try_get("", "warehouse_code")?,
        quantity: row.try_get("", "quantity")?,
        days_to_expiry: row.try_get("", "days_to_expiry").ok().flatten(),
    })
}

/// Lots in stock for an article (optionally one location), FEFO order.
pub async fn list_lot_stock<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: Option<i64>,
) -> AppResult<Vec<LotStock>> {
    let (filters, values): (&str, Vec<Value>) = match location_id {
        Some(loc) => (
            "AND l.article_id = ? AND t.location_id = ?",
            vec![article_id.into(), loc.into()],
        ),
        None => ("AND l.article_id = ?", vec![article_id.into()]),
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            lot_stock_sql(filters),
            values,
        ))
        .await?;
    rows.into_iter().map(map_lot_stock).collect()
}

/// Lots in stock that are expired or expire within `within_days` days (warning list).
pub async fn list_expiring_lots(db: &DatabaseConnection, within_days: i64) -> AppResult<Vec<LotStock>> {
    if within_days < 0 {
        return Err(AppError::ValidationFailed(vec!["within_days cannot be negative.".to_string()]));
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            lot_stock_sql("AND l.expiry_date IS NOT NULL AND l.expiry_date <= date('now', '+' || ? || ' days')"),
            [within_days.into()],
        ))
        .await?;
    rows.into_iter().map(map_lot_stock).collect()
}

/// Splits `quantity` of a tracked article at a location over its lots, FEFO, skipping expired
/// lots. Fails when the lots in stock (not expired) do not cover the quantity.
pub async fn fefo_allocate<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
    quantity: f64,
) -> AppResult<Vec<(i64, f64)>> {
    let lots = list_lot_stock(db, article_id, Some(location_id)).await?;
    let mut remaining = quantity;
    let mut allocation = Vec::new();
    let mut expired = 0.0;
    for lot in lots {
        if remaining <= EPS {
            break;
        }
        if lot.days_to_expiry.is_some_and(|d| d < 0) {
            expired += lot.quantity;
            continue;
        }
        let take = lot.quantity.min(remaining);
        allocation.push((lot.lot_id, take));
        remaining -= take;
    }
    if remaining > EPS {
        let mut message = format!(
            "Not enough lot-tracked stock at this location: {:.3} missing.",
            remaining
        );
        if expired > EPS {
            message.push_str(&format!(" {expired:.3} are in expired lots and are not issued automatically."));
        }
        return Err(AppError::ValidationFailed(vec![message]));
    }
    Ok(allocation)
}

/// Checks a chosen lot belongs to the article and holds `quantity` at the location.
pub async fn ensure_lot_available<C: ConnectionTrait>(
    db: &C,
    lot_id: i64,
    article_id: i64,
    location_id: i64,
    quantity: f64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_id, COALESCE(serial_number, lot_number) AS label FROM inventory_lots WHERE id = ?",
            [lot_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_lots".to_string(),
            id: lot_id.to_string(),
        })?;
    let lot_article: i64 = row.try_get("", "article_id")?;
    let label: String = row.try_get("", "label")?;
    if lot_article != article_id {
        return Err(AppError::ValidationFailed(vec![format!(
            "Lot {label} belongs to another article."
        )]));
    }
    let available = lot_stock_at(db, lot_id, location_id).await?;
    if available + EPS < quantity {
        return Err(AppError::ValidationFailed(vec![format!(
            "Lot {label} holds {available:.3} at this location, less than {quantity:.3}."
        )]));
    }
    Ok(())
}

/// Stamps a movement with its lot.
pub async fn set_movement_lot<C: ConnectionTrait>(db: &C, transaction_id: i64, lot_id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_transactions SET lot_id = ? WHERE id = ?",
        [lot_id.into(), transaction_id.into()],
    ))
    .await?;
    Ok(())
}

/// Serial-tracked quantities move by whole units.
pub fn ensure_whole_units(tracking: &ArticleTracking, quantity: f64) -> AppResult<()> {
    if tracking.serial && (quantity.fract().abs() > EPS) {
        return Err(AppError::ValidationFailed(vec![
            "Serial-tracked articles move by whole units.".to_string(),
        ]));
    }
    Ok(())
}
