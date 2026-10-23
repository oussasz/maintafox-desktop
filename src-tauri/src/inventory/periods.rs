//! Stock valuation reports and period close.
//!
//! **As-at valuation** replays the movement ledger up to a date (business date: `posting_date`,
//! else the date of `performed_at`) per article and location: inbound movements carrying a unit
//! cost blend into the weighted average, other inbound movements and all outbound movements only
//! change the quantity. Value = quantity × average; stock at a location that never had a cost is
//! reported as unvalued quantity, never given a value.
//!
//! **Period close** freezes the valuation at the period end: one snapshot line per article and
//! warehouse (`inventory_valuation_snapshot_lines`). Once a period is closed, any posting dated on
//! or before its end is refused (`ensure_posting_date_open`), and a report as at that end date
//! reads the frozen snapshot.

use std::collections::{BTreeMap, HashMap};

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    CloseValuationPeriodInput, InventoryValuationPeriod, ValuationReport, ValuationReportRow,
};
use crate::inventory::fx::{parse_iso_date, tenant_currency, today};
use crate::inventory::ledger::ON_HAND_MOVEMENTS;
use crate::inventory::valuation::weighted_moving_average;

const EPS: f64 = 1e-9;

/// Refuses a posting dated in (or before the end of) a closed valuation period.
pub async fn ensure_posting_date_open<C: ConnectionTrait>(db: &C, posting_date: &str) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT period_code, period_end FROM inventory_valuation_periods
             WHERE status = 'closed' AND period_end >= ?
             ORDER BY period_end DESC LIMIT 1",
            [posting_date.into()],
        ))
        .await?;
    if let Some(row) = row {
        let code: String = row.try_get("", "period_code")?;
        let end: String = row.try_get("", "period_end")?;
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INV_PERIOD_CLOSED: inventory period {code} is closed up to {end}; a posting dated \
             {posting_date} is refused. Post it with a date after {end}."
        )]));
    }
    Ok(())
}

/// Validates an optional back-dated posting date (not in the future, not in a closed period) and
/// returns the business date to use.
pub async fn resolve_posting_date<C: ConnectionTrait>(db: &C, posting_date: Option<&str>) -> AppResult<String> {
    let date = match posting_date.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => {
            parse_iso_date(d, "posting_date")?;
            if d > today().as_str() {
                return Err(AppError::ValidationFailed(vec![
                    "A posting cannot be dated in the future.".to_string(),
                ]));
            }
            d.to_string()
        }
        None => today(),
    };
    ensure_posting_date_open(db, &date).await?;
    Ok(date)
}

/// Records the business date of a back-dated movement.
pub async fn set_movement_posting_date<C: ConnectionTrait>(db: &C, transaction_id: i64, date: &str) -> AppResult<()> {
    if date == today() {
        return Ok(());
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_transactions SET posting_date = ? WHERE id = ?",
        [date.into(), transaction_id.into()],
    ))
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct Position {
    quantity: f64,
    average: Option<f64>,
}

/// Valuation per (article, location) as at `as_at` (inclusive), replayed from the ledger.
async fn replay_positions<C: ConnectionTrait>(db: &C, as_at: &str) -> AppResult<HashMap<(i64, i64), Position>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.article_id, t.location_id, t.movement_type, t.quantity, t.unit_cost,
                    COALESCE(t.posting_date, substr(t.performed_at, 1, 10)) AS business_date
             FROM inventory_transactions t
             WHERE COALESCE(t.posting_date, substr(t.performed_at, 1, 10)) <= ?
             ORDER BY business_date, t.id",
            [as_at.into()],
        ))
        .await?;
    let signs: HashMap<&str, f64> = ON_HAND_MOVEMENTS.iter().copied().collect();
    let mut positions: HashMap<(i64, i64), Position> = HashMap::new();
    for row in rows {
        let movement: String = row.try_get("", "movement_type")?;
        let Some(sign) = signs.get(movement.as_str()) else {
            continue; // reservations do not move on-hand stock
        };
        let article_id: i64 = row.try_get("", "article_id")?;
        let location_id: i64 = row.try_get("", "location_id")?;
        let quantity: f64 = row.try_get("", "quantity")?;
        let unit_cost: Option<f64> = row.try_get("", "unit_cost").ok().flatten();
        let delta = sign * quantity;
        let position = positions.entry((article_id, location_id)).or_default();
        if delta > 0.0 {
            if let Some(cost) = unit_cost.filter(|c| c.is_finite() && *c >= 0.0) {
                position.average = Some(weighted_moving_average(
                    position.quantity,
                    position.average,
                    delta,
                    cost,
                ));
            }
        }
        position.quantity += delta;
    }
    Ok(positions)
}

/// Article × warehouse figures (quantity, valued value, unvalued quantity).
#[derive(Debug, Clone, Default)]
struct WarehouseFigure {
    quantity: f64,
    value: f64,
    valued_qty: f64,
    unvalued_qty: f64,
}

async fn figures_as_at<C: ConnectionTrait>(db: &C, as_at: &str) -> AppResult<BTreeMap<(i64, i64), WarehouseFigure>> {
    let positions = replay_positions(db, as_at).await?;
    let locations = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, warehouse_id FROM stock_locations".to_string(),
        ))
        .await?;
    let mut warehouse_of: HashMap<i64, i64> = HashMap::new();
    for row in locations {
        warehouse_of.insert(row.try_get("", "id")?, row.try_get("", "warehouse_id")?);
    }
    let mut figures: BTreeMap<(i64, i64), WarehouseFigure> = BTreeMap::new();
    for ((article_id, location_id), position) in positions {
        if position.quantity <= EPS {
            continue;
        }
        let Some(warehouse_id) = warehouse_of.get(&location_id).copied() else {
            continue;
        };
        let figure = figures.entry((article_id, warehouse_id)).or_default();
        figure.quantity += position.quantity;
        match position.average {
            Some(avg) => {
                figure.value += position.quantity * avg;
                figure.valued_qty += position.quantity;
            }
            None => figure.unvalued_qty += position.quantity,
        }
    }
    Ok(figures)
}

/// Frozen snapshot of the closed period ending on `as_at`, if any.
async fn frozen_figures(
    db: &DatabaseConnection,
    as_at: &str,
) -> AppResult<Option<(String, BTreeMap<(i64, i64), WarehouseFigure>)>> {
    let period = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, period_code FROM inventory_valuation_periods
             WHERE status = 'closed' AND period_end = ? ORDER BY id DESC LIMIT 1",
            [as_at.into()],
        ))
        .await?;
    let Some(period) = period else {
        return Ok(None);
    };
    let period_id: i64 = period.try_get("", "id")?;
    let code: String = period.try_get("", "period_code")?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_id, warehouse_id, quantity, value, unvalued_qty
             FROM inventory_valuation_snapshot_lines WHERE period_id = ?",
            [period_id.into()],
        ))
        .await?;
    let mut figures = BTreeMap::new();
    for row in rows {
        let quantity: f64 = row.try_get("", "quantity")?;
        let unvalued: f64 = row.try_get("", "unvalued_qty")?;
        figures.insert(
            (row.try_get("", "article_id")?, row.try_get("", "warehouse_id")?),
            WarehouseFigure {
                quantity,
                value: row.try_get::<Option<f64>>("", "value")?.unwrap_or(0.0),
                valued_qty: quantity - unvalued,
                unvalued_qty: unvalued,
            },
        );
    }
    Ok(Some((code, figures)))
}

pub async fn valuation_report(
    db: &DatabaseConnection,
    as_at: Option<String>,
    group_by: String,
) -> AppResult<ValuationReport> {
    let as_at = as_at.unwrap_or_else(today);
    parse_iso_date(&as_at, "as_at")?;
    if !matches!(group_by.as_str(), "article" | "warehouse" | "family") {
        return Err(AppError::ValidationFailed(vec![
            "group_by must be article, warehouse or family.".to_string(),
        ]));
    }
    let currency_code = tenant_currency(db).await?;
    let (frozen_period_code, figures) = match frozen_figures(db, &as_at).await? {
        Some((code, figures)) => (Some(code), figures),
        None => (None, figures_as_at(db, &as_at).await?),
    };

    // Labels for the groups.
    let articles = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT a.id, a.article_code, a.article_name, a.family_id, af.code AS family_code, af.name AS family_name
             FROM articles a LEFT JOIN article_families af ON af.id = a.family_id"
                .to_string(),
        ))
        .await?;
    let mut article_info: HashMap<i64, (String, String, Option<i64>, Option<String>, Option<String>)> = HashMap::new();
    for row in articles {
        article_info.insert(
            row.try_get("", "id")?,
            (
                row.try_get("", "article_code")?,
                row.try_get("", "article_name")?,
                row.try_get("", "family_id")?,
                row.try_get("", "family_code")?,
                row.try_get("", "family_name")?,
            ),
        );
    }
    let warehouses = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name FROM warehouses".to_string(),
        ))
        .await?;
    let mut warehouse_info: HashMap<i64, (String, String)> = HashMap::new();
    for row in warehouses {
        warehouse_info.insert(
            row.try_get("", "id")?,
            (row.try_get("", "code")?, row.try_get("", "name")?),
        );
    }

    struct Acc {
        code: String,
        name: String,
        quantity: f64,
        value: f64,
        unvalued_qty: f64,
        articles: std::collections::HashSet<i64>,
    }
    let mut groups: BTreeMap<String, (Option<i64>, Acc)> = BTreeMap::new();
    let mut total_value = 0.0;
    let mut total_unvalued = 0.0;
    for ((article_id, warehouse_id), figure) in &figures {
        let (a_code, a_name, family_id, family_code, family_name) =
            article_info.get(article_id).cloned().unwrap_or_default();
        let (key, group_id, code, name) = match group_by.as_str() {
            "article" => (format!("a{article_id}"), Some(*article_id), a_code, a_name),
            "warehouse" => {
                let (w_code, w_name) = warehouse_info.get(warehouse_id).cloned().unwrap_or_default();
                (format!("w{warehouse_id}"), Some(*warehouse_id), w_code, w_name)
            }
            _ => match family_id {
                Some(fid) => (
                    format!("f{fid}"),
                    Some(fid),
                    family_code.unwrap_or_default(),
                    family_name.unwrap_or_default(),
                ),
                None => ("f-none".to_string(), None, String::new(), String::new()),
            },
        };
        let entry = groups.entry(key).or_insert_with(|| {
            (
                group_id,
                Acc {
                    code,
                    name,
                    quantity: 0.0,
                    value: 0.0,
                    unvalued_qty: 0.0,
                    articles: Default::default(),
                },
            )
        });
        entry.1.quantity += figure.quantity;
        entry.1.value += figure.value;
        entry.1.unvalued_qty += figure.unvalued_qty;
        entry.1.articles.insert(*article_id);
        total_value += figure.value;
        total_unvalued += figure.unvalued_qty;
    }
    let mut rows: Vec<ValuationReportRow> = groups
        .into_values()
        .map(|(group_id, acc)| ValuationReportRow {
            group_id,
            group_code: acc.code,
            group_name: acc.name,
            quantity: acc.quantity,
            value: acc.value,
            unvalued_qty: acc.unvalued_qty,
            article_count: acc.articles.len() as i64,
        })
        .collect();
    rows.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    Ok(ValuationReport {
        as_at,
        group_by,
        currency_code,
        frozen_period_code,
        total_value,
        unvalued_qty: total_unvalued,
        rows,
    })
}

fn map_period(row: sea_orm::QueryResult) -> AppResult<InventoryValuationPeriod> {
    Ok(InventoryValuationPeriod {
        id: row.try_get("", "id")?,
        period_code: row.try_get("", "period_code")?,
        period_start: row.try_get("", "period_start")?,
        period_end: row.try_get("", "period_end")?,
        status: row.try_get("", "status")?,
        currency_code: row.try_get("", "currency_code")?,
        total_value: row.try_get("", "total_value")?,
        unvalued_qty: row.try_get("", "unvalued_qty")?,
        notes: row.try_get("", "notes")?,
        closed_by_name: row.try_get("", "closed_by_name").ok().flatten(),
        closed_at: row.try_get("", "closed_at")?,
    })
}

const PERIOD_SELECT: &str = "SELECT p.id, p.period_code, p.period_start, p.period_end, p.status, p.currency_code,
            p.total_value, p.unvalued_qty, p.notes, COALESCE(ua.display_name, ua.username) AS closed_by_name,
            p.closed_at
     FROM inventory_valuation_periods p
     LEFT JOIN user_accounts ua ON ua.id = p.closed_by_id";

pub async fn list_periods(db: &DatabaseConnection) -> AppResult<Vec<InventoryValuationPeriod>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{PERIOD_SELECT} ORDER BY p.period_end DESC, p.id DESC"),
        ))
        .await?;
    rows.into_iter().map(map_period).collect()
}

/// Calendar month `YYYY-MM` when the period is exactly one month, else `start..end`.
fn period_code(start: chrono::NaiveDate, end: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let month_end = if start.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)
    }
    .and_then(|d| d.pred_opt());
    if start.day() == 1 && Some(end) == month_end {
        format!("{:04}-{:02}", start.year(), start.month())
    } else {
        format!("{start}..{end}")
    }
}

pub async fn close_period(
    db: &DatabaseConnection,
    input: CloseValuationPeriodInput,
) -> AppResult<InventoryValuationPeriod> {
    let start = parse_iso_date(&input.period_start, "period_start")?;
    let end = parse_iso_date(&input.period_end, "period_end")?;
    if start > end {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_PERIOD_RANGE: The period start must be on or before its end.".to_string(),
        ]));
    }
    let today_date = Utc::now().date_naive();
    if end >= today_date {
        return Err(AppError::ValidationFailed(vec![format!(
            "A period can be closed once it has ended: {end} is not in the past."
        )]));
    }
    let start_s = start.to_string();
    let end_s = end.to_string();
    let overlap = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT period_code FROM inventory_valuation_periods
             WHERE status = 'closed' AND period_end >= ? ORDER BY period_end DESC LIMIT 1",
            [start_s.clone().into()],
        ))
        .await?;
    if let Some(row) = overlap {
        let code: String = row.try_get("", "period_code")?;
        return Err(AppError::ValidationFailed(vec![format!(
            "Period {code} is already closed on or after {start_s}: periods are closed in order and never overlap."
        )]));
    }
    let currency_code = tenant_currency(db).await?;
    let code = period_code(start, end);
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let tx = db.begin().await?;
    let figures = figures_as_at(&tx, &end_s).await?;
    let total_value: f64 = figures.values().map(|f| f.value).sum();
    let unvalued_qty: f64 = figures.values().map(|f| f.unvalued_qty).sum();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_valuation_periods
            (entity_sync_id, period_code, period_start, period_end, status, currency_code, total_value,
             unvalued_qty, notes, closed_by_id, closed_at, row_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'closed', ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            code.into(),
            start_s.into(),
            end_s.into(),
            currency_code.into(),
            total_value.into(),
            unvalued_qty.into(),
            input
                .notes
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .map_or(Value::String(None), Value::from),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now.clone().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;
    let period_id: i64 = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read period id")))?
        .try_get("", "id")?;
    for ((article_id, warehouse_id), figure) in &figures {
        let unit_cost = (figure.valued_qty > EPS).then(|| figure.value / figure.valued_qty);
        let value = (figure.valued_qty > EPS).then_some(figure.value);
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_valuation_snapshot_lines
                (entity_sync_id, period_id, article_id, warehouse_id, quantity, unit_cost, value, unvalued_qty, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                uuid::Uuid::new_v4().to_string().into(),
                period_id.into(),
                (*article_id).into(),
                (*warehouse_id).into(),
                figure.quantity.into(),
                unit_cost.map_or(Value::Double(None), Value::from),
                value.map_or(Value::Double(None), Value::from),
                figure.unvalued_qty.into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }
    tx.commit().await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{PERIOD_SELECT} WHERE p.id = ?"),
            [period_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("closed period not found")))?;
    map_period(row)
}

#[cfg(test)]
mod tests {
    use super::period_code;

    #[test]
    fn period_code_names_calendar_months() {
        let d = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(period_code(d("2026-09-01"), d("2026-09-30")), "2026-09");
        assert_eq!(period_code(d("2026-12-01"), d("2026-12-31")), "2026-12");
        assert_eq!(period_code(d("2026-02-01"), d("2026-02-28")), "2026-02");
        assert_eq!(period_code(d("2026-09-01"), d("2026-09-15")), "2026-09-01..2026-09-15");
    }
}
