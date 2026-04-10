//! WO parts tracking — planned vs. actual consumption per work order.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! `work_order_parts` separates planned quantities (set during planning) from
//! actual quantities used (recorded during execution or at mechanical completion).
//!
//! Business rules:
//!   - Planned parts may be added any time the WO is not closed/cancelled.
//!   - Actual usage (`quantity_used`) may only be recorded when the WO is
//!     in_progress or mechanically_complete.
//!   - The parts quality gate in `complete_wo_mechanically` requires at least one
//!     row with `quantity_used > 0`, OR the WO's `parts_actuals_confirmed = 1` flag
//!     set via `confirm_no_parts_used`.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoPart {
    pub id: i64,
    pub work_order_id: i64,
    pub article_id: Option<i64>,
    pub article_ref: Option<String>,
    pub quantity_planned: f64,
    pub quantity_used: Option<f64>,
    pub unit_cost: Option<f64>,
    pub stock_location_id: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddPartInput {
    pub wo_id: i64,
    pub article_id: Option<i64>,
    pub article_ref: Option<String>,
    pub quantity_planned: f64,
    pub unit_cost: Option<f64>,
    pub notes: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WoPart row decode error for '{field}': {e}"
    ))
}

fn map_part(row: &sea_orm::QueryResult) -> AppResult<WoPart> {
    Ok(WoPart {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        article_id: row
            .try_get::<Option<i64>>("", "article_id")
            .map_err(|e| decode_err("article_id", e))?,
        article_ref: row
            .try_get::<Option<String>>("", "article_ref")
            .map_err(|e| decode_err("article_ref", e))?,
        quantity_planned: row
            .try_get::<f64>("", "quantity_planned")
            .map_err(|e| decode_err("quantity_planned", e))?,
        quantity_used: row
            .try_get::<Option<f64>>("", "quantity_used")
            .map_err(|e| decode_err("quantity_used", e))?,
        unit_cost: row
            .try_get::<Option<f64>>("", "unit_cost")
            .map_err(|e| decode_err("unit_cost", e))?,
        stock_location_id: row
            .try_get::<Option<i64>>("", "stock_location_id")
            .map_err(|e| decode_err("stock_location_id", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

/// Load the WO status code.
async fn load_wo_status_code(db: &DatabaseConnection, wo_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code \
             FROM work_orders wo \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    row.try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))
}

const PART_COLS: &str =
    "id, work_order_id, article_id, article_ref, quantity_planned, quantity_used, unit_cost, stock_location_id, notes";

// ═══════════════════════════════════════════════════════════════════════════════
// A) add_planned_part
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn add_planned_part(
    db: &DatabaseConnection,
    input: AddPartInput,
) -> AppResult<WoPart> {
    let status_code = load_wo_status_code(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible d'ajouter des pièces à un OT {status_code}."
        )]));
    }

    if input.quantity_planned < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity_planned doit être >= 0.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_parts \
         (work_order_id, article_id, article_ref, quantity_planned, unit_cost, notes) \
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            input
                .article_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            input
                .article_ref
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input.quantity_planned.into(),
            input
                .unit_cost
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            input
                .notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {PART_COLS} FROM work_order_parts WHERE rowid = last_insert_rowid()"),
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Failed to re-read part after insert"))
        })?;
    map_part(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) record_actual_usage
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn record_actual_usage(
    db: &DatabaseConnection,
    wo_part_id: i64,
    quantity_used: f64,
    unit_cost: Option<f64>,
) -> AppResult<WoPart> {
    // Load the part to get wo_id
    let part_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {PART_COLS} FROM work_order_parts WHERE id = ?"),
            [wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: wo_part_id.to_string(),
        })?;
    let part = map_part(&part_row)?;

    let status_code = load_wo_status_code(db, part.work_order_id).await?;
    if !matches!(
        status_code.as_str(),
        "in_progress" | "mechanically_complete"
    ) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Les réels des pièces ne peuvent être saisis qu'au statut 'in_progress' ou \
             'mechanically_complete'. Statut actuel : '{status_code}'."
        )]));
    }

    if quantity_used < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity_used doit être >= 0.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts SET \
            quantity_used = ?, \
            unit_cost = COALESCE(?, unit_cost) \
         WHERE id = ?",
        [
            quantity_used.into(),
            unit_cost
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            wo_part_id.into(),
        ],
    ))
    .await?;

    let updated = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {PART_COLS} FROM work_order_parts WHERE id = ?"),
            [wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: wo_part_id.to_string(),
        })?;
    map_part(&updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) confirm_no_parts_used
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn confirm_no_parts_used(
    db: &DatabaseConnection,
    wo_id: i64,
    _actor_id: i64,
) -> AppResult<()> {
    let rows = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET parts_actuals_confirmed = 1 WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;

    if rows.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) list_wo_parts
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_wo_parts(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoPart>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {PART_COLS} FROM work_order_parts WHERE work_order_id = ? ORDER BY id ASC"
            ),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_part).collect()
}
