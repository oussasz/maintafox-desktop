//! Valuation policy engine
use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValuationCostResult {
    pub unit_cost: f64,
    pub currency_value_id: i64,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub effective_at: String,
    pub is_provisional: bool,
    pub confidence: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplenishmentProjection {
    pub unit_cost: f64,
    pub extended_cost: f64,
    pub currency_value_id: i64,
    pub confidence: String,
    pub basis: String,
}

pub async fn evaluate_unit_cost(
    db: &impl ConnectionTrait,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
) -> AppResult<ValuationCostResult> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT valuation_method, currency_value_id, standard_unit_cost, scope_level, warehouse_id, family_id, article_id
             FROM inventory_valuation_policies WHERE is_active = 1
             ORDER BY scope_level DESC, sort_order ASC, id ASC"
                .to_string(),
        ))
        .await?;
    let fam: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT family_id FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "family_id").ok())
        .flatten();

    for row in rows {
        let scope: i32 = row.try_get("", "scope_level")?;
        let wh: Option<i64> = row.try_get("", "warehouse_id")?;
        let fa: Option<i64> = row.try_get("", "family_id")?;
        let ar: Option<i64> = row.try_get("", "article_id")?;
        let ok = match scope {
            0 => wh.is_none() && fa.is_none() && ar.is_none(),
            1 => wh == Some(warehouse_id) && fa.is_none() && ar.is_none(),
            2 => fa.is_some() && fa == fam && wh.is_none() && ar.is_none(),
            3 => fa.is_some() && fa == fam && wh == Some(warehouse_id) && ar.is_none(),
            4 => ar == Some(article_id) && wh.is_none(),
            5 => ar == Some(article_id) && wh == Some(warehouse_id),
            _ => false,
        };
        if !ok {
            continue;
        }
        let method: String = row.try_get("", "valuation_method")?;
        let currency_value_id: i64 = row.try_get("", "currency_value_id")?;
        let standard: Option<f64> = row.try_get("", "standard_unit_cost")?;
        let eff = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        return match method.as_str() {
            "STANDARD" => {
                let u = standard.unwrap_or(0.0);
                Ok(ValuationCostResult {
                    unit_cost: u,
                    currency_value_id,
                    source_type: "STANDARD".to_string(),
                    source_ref: None,
                    effective_at: eff,
                    is_provisional: u == 0.0,
                    confidence: if u > 0.0 {
                        "medium".to_string()
                    } else {
                        "low".to_string()
                    },
                })
            }
            "MOVING_AVG" => {
                let b = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT moving_avg_unit_cost FROM stock_balances WHERE article_id = ? AND location_id = ?",
                        [article_id.into(), location_id.into()],
                    ))
                    .await?;
                let avg: f64 = b
                    .and_then(|r| r.try_get::<Option<f64>>("", "moving_avg_unit_cost").ok())
                    .flatten()
                    .unwrap_or(0.0);
                Ok(ValuationCostResult {
                    unit_cost: avg,
                    currency_value_id,
                    source_type: "MOVING_AVG".to_string(),
                    source_ref: Some(format!("loc:{location_id}")),
                    effective_at: eff,
                    is_provisional: avg == 0.0,
                    confidence: if avg > 0.0 {
                        "medium".to_string()
                    } else {
                        "low".to_string()
                    },
                })
            }
            "LAST_RECEIPT" => {
                let r = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT pol.unit_price AS up, gr.gr_number
                         FROM goods_receipt_lines grl
                         JOIN goods_receipts gr ON gr.id = grl.goods_receipt_id
                         JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
                         WHERE grl.article_id = ? AND grl.accepted_qty > 0 AND pol.unit_price IS NOT NULL
                         ORDER BY grl.id DESC LIMIT 1",
                        [article_id.into()],
                    ))
                    .await?;
                let Some(r) = r else {
                    continue;
                };
                let up: f64 = r.try_get("", "up")?;
                let gr: String = r.try_get("", "gr_number")?;
                Ok(ValuationCostResult {
                    unit_cost: up,
                    currency_value_id,
                    source_type: "LAST_RECEIPT".to_string(),
                    source_ref: Some(gr),
                    effective_at: eff,
                    is_provisional: false,
                    confidence: "high".to_string(),
                })
            }
            "CONTRACT" => {
                let u = standard.unwrap_or(0.0);
                Ok(ValuationCostResult {
                    unit_cost: u,
                    currency_value_id,
                    source_type: "CONTRACT".to_string(),
                    source_ref: None,
                    effective_at: eff,
                    is_provisional: u == 0.0,
                    confidence: "medium".to_string(),
                })
            }
            _ => Err(AppError::ValidationFailed(vec![format!(
                "GATE_INV_VALUATION_METHOD_INVALID: Bad method {method}"
            )])),
        };
    }
    Err(AppError::ValidationFailed(vec![
        "GATE_INV_VALUATION_POLICY_MISSING: No matching valuation policy.".to_string(),
    ]))
}

pub async fn project_replenishment_cost(
    db: &DatabaseConnection,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    requested_qty: f64,
) -> AppResult<ReplenishmentProjection> {
    let ev = evaluate_unit_cost(db, article_id, warehouse_id, location_id).await?;
    Ok(ReplenishmentProjection {
        extended_cost: ev.unit_cost * requested_qty,
        unit_cost: ev.unit_cost,
        currency_value_id: ev.currency_value_id,
        confidence: ev.confidence.clone(),
        basis: format!("{}:{:?}", ev.source_type, ev.source_ref),
    })
}

pub fn weighted_moving_average(old_on_hand: f64, old_avg: Option<f64>, qty_in: f64, unit_in: f64) -> f64 {
    let oa = old_on_hand.max(0.0);
    let oavg = old_avg.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(unit_in);
    if oa + qty_in <= f64::EPSILON {
        return unit_in;
    }
    (oa * oavg + qty_in * unit_in) / (oa + qty_in)
}

/// Cost provenance of a costed stock movement (`inventory_transactions.cost_source_type`).
pub const COST_SOURCE_MANUAL_ENTRY: &str = "MANUAL_ENTRY";
pub const COST_SOURCE_PO_RECEIPT: &str = "PO_RECEIPT";
pub const COST_SOURCE_MOVING_AVG: &str = "MOVING_AVG";

/// Rejects a negative or non-finite unit cost.
pub fn validate_unit_cost(unit_cost: f64) -> AppResult<()> {
    if !unit_cost.is_finite() || unit_cost < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_PRICE_NEGATIVE: unit_cost must be a number greater than or equal to 0.".to_string(),
        ]));
    }
    Ok(())
}

/// Current weighted-average unit cost of an article at one location (None when never costed).
pub async fn location_average_cost<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<Option<f64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT moving_avg_unit_cost FROM stock_balances WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<Option<f64>>("", "moving_avg_unit_cost").ok())
        .flatten())
}

/// Writes the cost of one movement on its ledger row (`inventory_transactions` is synced, so the
/// cost travels with the movement; `stock_balances` averages are a local projection).
pub async fn stamp_movement_cost<C: ConnectionTrait>(
    db: &C,
    transaction_id: i64,
    quantity: f64,
    unit_cost: f64,
    cost_source_type: &str,
    cost_source_ref: Option<&str>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_transactions
         SET unit_cost = ?, extended_cost = ?, cost_source_type = ?, cost_source_ref = ?,
             cost_effective_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), is_provisional = 0
         WHERE id = ?",
        [
            unit_cost.into(),
            (unit_cost * quantity.abs()).into(),
            cost_source_type.to_string().into(),
            cost_source_ref.map(str::to_string).into(),
            transaction_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Inbound movement at a known unit cost: recompute the location's weighted average from the
/// on-hand **before** the movement, store it on the balance row (which must already exist) and
/// stamp the movement. Returns the new average.
#[allow(clippy::too_many_arguments)]
pub async fn apply_inbound_cost<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
    prior_on_hand: f64,
    qty_in: f64,
    unit_cost: f64,
    transaction_id: i64,
    cost_source_type: &str,
    cost_source_ref: Option<&str>,
) -> AppResult<f64> {
    validate_unit_cost(unit_cost)?;
    let prior_avg = location_average_cost(db, article_id, location_id).await?;
    let new_avg = weighted_moving_average(prior_on_hand, prior_avg, qty_in, unit_cost);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_balances SET moving_avg_unit_cost = ? WHERE article_id = ? AND location_id = ?",
        [new_avg.into(), article_id.into(), location_id.into()],
    ))
    .await?;
    stamp_movement_cost(db, transaction_id, qty_in, unit_cost, cost_source_type, cost_source_ref).await?;
    Ok(new_avg)
}

/// Movement valued at the location's current average (outbound, or inbound without its own cost);
/// the average itself does not change. Leaves the movement uncosted when the location has never
/// been costed.
pub async fn stamp_at_average<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
    quantity: f64,
    transaction_id: i64,
) -> AppResult<Option<f64>> {
    let avg = location_average_cost(db, article_id, location_id).await?;
    if let Some(avg) = avg {
        stamp_movement_cost(db, transaction_id, quantity, avg, COST_SOURCE_MOVING_AVG, None).await?;
    }
    Ok(avg)
}
