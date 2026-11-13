//! Dormant stock: articles still on hand whose last issue is older than a chosen number of
//! months (or that were never issued and have been in the catalogue that long). The storekeeper
//! uses it to find obsolete parts that tie up money.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DormantStockRow {
    pub article_id: i64,
    pub article_code: String,
    pub article_name: String,
    pub on_hand_qty: f64,
    /// Value at the moving average cost; `None` when no balance has a known cost.
    pub stock_value: Option<f64>,
    /// Last issue to a work order or cost centre; `None` when the article was never issued.
    pub last_issue_at: Option<String>,
    /// Last movement of any kind (receipt, transfer, count…), shown for context.
    pub last_movement_at: Option<String>,
    pub article_created_at: String,
}

/// Months a user may choose; anything else is clamped into this range.
pub const MIN_MONTHS: i64 = 1;
pub const MAX_MONTHS: i64 = 120;

/// Active articles with stock on hand and no issue since `months` months ago, most valuable
/// first. An article never issued counts as dormant once it is older than the same period.
pub async fn list_dormant_stock<C: ConnectionTrait>(db: &C, months: i64) -> AppResult<Vec<DormantStockRow>> {
    let months = months.clamp(MIN_MONTHS, MAX_MONTHS);
    let modifier = format!("-{months} months");
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH bal AS (
                 SELECT article_id,
                        SUM(on_hand_qty) AS on_hand_qty,
                        SUM(CASE WHEN moving_avg_unit_cost IS NOT NULL
                                 THEN on_hand_qty * moving_avg_unit_cost END) AS stock_value
                 FROM stock_balances
                 GROUP BY article_id
             ),
             mv AS (
                 SELECT article_id,
                        MAX(CASE WHEN movement_type IN ('ISSUE', 'SORTIE_OT') THEN performed_at END) AS last_issue_at,
                        MAX(performed_at) AS last_movement_at
                 FROM inventory_transactions
                 GROUP BY article_id
             )
             SELECT a.id AS article_id, a.article_code, a.article_name,
                    CAST(bal.on_hand_qty AS REAL) AS on_hand_qty,
                    CAST(bal.stock_value AS REAL) AS stock_value,
                    mv.last_issue_at, mv.last_movement_at,
                    a.created_at AS article_created_at
             FROM articles a
             JOIN bal ON bal.article_id = a.id
             LEFT JOIN mv ON mv.article_id = a.id
             WHERE a.is_active = 1
               AND bal.on_hand_qty > 0
               AND COALESCE(mv.last_issue_at, a.created_at)
                   < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?)
             ORDER BY COALESCE(bal.stock_value, 0) DESC, a.article_code ASC",
            [modifier.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(DormantStockRow {
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                stock_value: row.try_get("", "stock_value").ok().flatten(),
                last_issue_at: row.try_get("", "last_issue_at").ok().flatten(),
                last_movement_at: row.try_get("", "last_movement_at").ok().flatten(),
                article_created_at: row.try_get("", "article_created_at")?,
            })
        })
        .collect()
}
