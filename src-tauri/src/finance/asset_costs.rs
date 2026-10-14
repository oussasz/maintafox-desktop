//! Maintenance cost history per asset vs its replacement asset value (RAV).
//!
//! Reads the cost columns of closed OTs, which the settlement keeps equal to the settled figures
//! (`settlement::write_back_wo_costs`), so this view, the asset record, SMRP cost/RAV and the RAMS
//! cost-of-failure all agree. Corrective = OT types `corrective` / `emergency` (failure cost, used
//! by RAMS cost-based intervals); preventive = every other type.

use chrono::{Datelike, Duration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;
use crate::finance::wo_costing::{decode, round_money};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetCostFilter {
    pub search: Option<String>,
    pub cost_center_code: Option<String>,
    /// Only assets with at least one costed closed OT (default true).
    pub only_with_cost: Option<bool>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetCostYear {
    pub year: i64,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetCostRow {
    pub equipment_id: i64,
    pub equipment_code: String,
    pub equipment_name: String,
    pub cost_center_code: Option<String>,
    pub replacement_value: Option<f64>,
    pub lifetime_cost: f64,
    pub lifetime_labor: f64,
    pub lifetime_parts: f64,
    pub lifetime_services: f64,
    pub cost_12m: f64,
    pub closed_work_orders: i64,
    pub corrective_work_orders: i64,
    pub corrective_cost: f64,
    pub preventive_cost: f64,
    /// Mean cost of a corrective OT (failure cost basis for RAMS); None without corrective OT.
    pub avg_corrective_cost: Option<f64>,
    pub avg_preventive_cost: Option<f64>,
    /// 12-month cost / RAV × 100 (SMRP cost/RAV at asset level); None without RAV.
    pub cost_12m_to_rav_pct: Option<f64>,
    pub lifetime_to_rav_pct: Option<f64>,
    pub last_closed_at: Option<String>,
    /// Last five calendar years, oldest first.
    pub by_year: Vec<AssetCostYear>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetCostHistory {
    pub currency_code: Option<String>,
    pub rows: Vec<AssetCostRow>,
    pub fleet_replacement_value: f64,
    pub fleet_cost_12m: f64,
    /// Σ 12-month cost on valued assets / Σ RAV × 100.
    pub fleet_cost_to_rav_pct: Option<f64>,
    pub assets_without_rav: i64,
}

pub async fn list_asset_cost_history<C: ConnectionTrait>(db: &C, filter: AssetCostFilter) -> AppResult<AssetCostHistory> {
    let now = Utc::now();
    let since_12m = (now - Duration::days(365)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let this_year = i64::from(now.year());
    let search = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));
    let cc = filter
        .cost_center_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let only_with_cost = filter.only_with_cost.unwrap_or(true);
    let limit = filter.limit.unwrap_or(200).clamp(1, 2000);

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, COALESCE(e.asset_id_code, e.code) AS code, e.name, e.cost_center_code, e.replacement_value,
                    COUNT(wo.id) AS closed_wos,
                    COALESCE(SUM(wo.total_cost), 0.0) AS lifetime_cost,
                    COALESCE(SUM(wo.labor_cost), 0.0) AS lifetime_labor,
                    COALESCE(SUM(wo.parts_cost), 0.0) AS lifetime_parts,
                    COALESCE(SUM(wo.service_cost), 0.0) AS lifetime_services,
                    COALESCE(SUM(CASE WHEN wo.closed_at >= ?1 THEN wo.total_cost END), 0.0) AS cost_12m,
                    COALESCE(SUM(CASE WHEN wot.code IN ('corrective', 'emergency') THEN 1 ELSE 0 END), 0) AS corrective_wos,
                    COALESCE(SUM(CASE WHEN wot.code IN ('corrective', 'emergency') THEN wo.total_cost END), 0.0) AS corrective_cost,
                    COALESCE(SUM(CASE WHEN wo.id IS NOT NULL AND (wot.code IS NULL OR wot.code NOT IN ('corrective', 'emergency'))
                                      THEN wo.total_cost END), 0.0) AS preventive_cost,
                    MAX(wo.closed_at) AS last_closed_at
             FROM equipment e
             LEFT JOIN work_orders wo ON wo.equipment_id = e.id AND wo.closed_at IS NOT NULL
             LEFT JOIN work_order_types wot ON wot.id = wo.type_id
             WHERE e.deleted_at IS NULL
               AND (?2 IS NULL OR lower(COALESCE(e.asset_id_code, '') || ' ' || e.code || ' ' || e.name) LIKE ?2)
               AND (?3 IS NULL OR e.cost_center_code = ?3)
             GROUP BY e.id
             HAVING (?4 = 0 OR COALESCE(SUM(wo.total_cost), 0.0) > 0)
             ORDER BY lifetime_cost DESC, code ASC
             LIMIT ?5",
            [
                since_12m.clone().into(),
                search.into(),
                cc.into(),
                i64::from(only_with_cost).into(),
                limit.into(),
            ],
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let equipment_id: i64 = decode(&r, "id")?;
        let replacement_value: Option<f64> = decode::<Option<f64>>(&r, "replacement_value")?.filter(|v| *v > 0.0);
        let lifetime_cost: f64 = decode(&r, "lifetime_cost")?;
        let cost_12m: f64 = decode(&r, "cost_12m")?;
        let closed: i64 = decode(&r, "closed_wos")?;
        let corrective: i64 = decode(&r, "corrective_wos")?;
        let corrective_cost: f64 = decode(&r, "corrective_cost")?;
        let preventive_cost: f64 = decode(&r, "preventive_cost")?;
        let years = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT CAST(strftime('%Y', closed_at) AS INTEGER) AS y, COALESCE(SUM(total_cost), 0.0) AS amount
                 FROM work_orders WHERE equipment_id = ? AND closed_at IS NOT NULL
                   AND CAST(strftime('%Y', closed_at) AS INTEGER) > ?
                 GROUP BY y",
                [equipment_id.into(), (this_year - 5).into()],
            ))
            .await?;
        let mut by_year: Vec<AssetCostYear> = ((this_year - 4)..=this_year)
            .map(|year| AssetCostYear { year, amount: 0.0 })
            .collect();
        for y in years {
            let year: i64 = decode(&y, "y")?;
            if let Some(slot) = by_year.iter_mut().find(|s| s.year == year) {
                slot.amount = round_money(decode(&y, "amount")?);
            }
        }
        out.push(AssetCostRow {
            equipment_id,
            equipment_code: decode(&r, "code")?,
            equipment_name: decode(&r, "name")?,
            cost_center_code: decode(&r, "cost_center_code")?,
            replacement_value,
            lifetime_cost: round_money(lifetime_cost),
            lifetime_labor: round_money(decode(&r, "lifetime_labor")?),
            lifetime_parts: round_money(decode(&r, "lifetime_parts")?),
            lifetime_services: round_money(decode(&r, "lifetime_services")?),
            cost_12m: round_money(cost_12m),
            closed_work_orders: closed,
            corrective_work_orders: corrective,
            corrective_cost: round_money(corrective_cost),
            preventive_cost: round_money(preventive_cost),
            avg_corrective_cost: (corrective > 0).then(|| round_money(corrective_cost / corrective as f64)),
            avg_preventive_cost: (closed > corrective)
                .then(|| round_money(preventive_cost / (closed - corrective) as f64)),
            cost_12m_to_rav_pct: replacement_value.map(|rav| round_money(cost_12m / rav * 100.0)),
            lifetime_to_rav_pct: replacement_value.map(|rav| round_money(lifetime_cost / rav * 100.0)),
            last_closed_at: decode(&r, "last_closed_at")?,
            by_year,
        });
    }

    let fleet = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE WHEN e.replacement_value > 0 THEN e.replacement_value END), 0.0) AS rav,
                    COALESCE(SUM(CASE WHEN COALESCE(e.replacement_value, 0) <= 0 THEN 1 ELSE 0 END), 0) AS without_rav,
                    (SELECT COALESCE(SUM(wo.total_cost), 0.0) FROM work_orders wo
                       JOIN equipment e2 ON e2.id = wo.equipment_id
                      WHERE wo.closed_at >= ? AND e2.replacement_value > 0 AND e2.deleted_at IS NULL) AS cost_12m
             FROM equipment e WHERE e.deleted_at IS NULL",
            [since_12m.into()],
        ))
        .await?;
    let (rav, without_rav, fleet_cost) = match fleet {
        Some(f) => (
            decode::<f64>(&f, "rav")?,
            decode::<i64>(&f, "without_rav")?,
            decode::<f64>(&f, "cost_12m")?,
        ),
        None => (0.0, 0, 0.0),
    };
    Ok(AssetCostHistory {
        currency_code: crate::inventory::fx::tenant_currency(db).await.ok(),
        rows: out,
        fleet_replacement_value: round_money(rav),
        fleet_cost_12m: round_money(fleet_cost),
        fleet_cost_to_rav_pct: (rav > 0.0).then(|| round_money(fleet_cost / rav * 100.0)),
        assets_without_rav: without_rav,
    })
}
