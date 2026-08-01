//! Valuation policy engine
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use crate::errors::{AppError, AppResult};

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
    db: &DatabaseConnection,
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
                    confidence: if u > 0.0 { "medium".to_string() } else { "low".to_string() },
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
                    confidence: if avg > 0.0 { "medium".to_string() } else { "low".to_string() },
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
            _ => Err(AppError::ValidationFailed(vec![format!("Bad method {method}")])),
        };
    }
    Err(AppError::ValidationFailed(vec!["No matching valuation policy.".to_string()]))
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
