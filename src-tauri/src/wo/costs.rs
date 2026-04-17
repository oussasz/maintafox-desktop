//! WO cost summary and posting hook for SP §6.24 Budget/Cost integration.
//!
//! Phase 2 - Sub-phase 05 - File 03 - Sprint S1.
//!
//! Functions:
//!   get_cost_summary      — computed cost breakdown with duration variance
//!   get_cost_posting_hook — structured payload consumed by Budget module
//!   update_service_cost   — manual vendor/service cost entry

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Computed cost breakdown for a work order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoCostSummary {
    pub wo_id: i64,
    pub labor_cost: f64,
    pub parts_cost: f64,
    pub service_cost: f64,
    pub total_cost: f64,
    pub expected_duration_hours: Option<f64>,
    pub actual_duration_hours: Option<f64>,
    pub active_labor_hours: f64,
    pub total_waiting_hours: f64,
    /// actual - expected (positive = overrun).
    pub duration_variance_hours: Option<f64>,
}

/// Structured payload for Budget/Cost §6.24 consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostPostingHook {
    pub wo_id: i64,
    pub wo_code: String,
    pub entity_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub type_code: String,
    pub urgency_level: Option<i64>,
    pub total_cost: f64,
    pub labor_cost: f64,
    pub parts_cost: f64,
    pub service_cost: f64,
    pub closed_at: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WO cost row decode failed for column '{column}': {e}"
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) get_cost_summary
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the cost summary for a WO from sub-entity accumulators.
pub async fn get_cost_summary(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<WoCostSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                wo.id AS wo_id, \
                COALESCE(wo.labor_cost, 0) AS labor_cost, \
                COALESCE(wo.parts_cost, 0) AS parts_cost, \
                COALESCE(wo.service_cost, COALESCE(wo.service_cost_input, 0)) AS service_cost, \
                COALESCE(wo.total_cost, 0) AS total_cost, \
                wo.expected_duration_hours, \
                wo.actual_duration_hours, \
                COALESCE(wo.active_labor_hours, 0) AS active_labor_hours, \
                COALESCE(wo.total_waiting_hours, 0) AS total_waiting_hours \
             FROM work_orders wo WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    let expected: Option<f64> = row
        .try_get::<Option<f64>>("", "expected_duration_hours")
        .map_err(|e| decode_err("expected_duration_hours", e))?;
    let actual: Option<f64> = row
        .try_get::<Option<f64>>("", "actual_duration_hours")
        .map_err(|e| decode_err("actual_duration_hours", e))?;

    let variance = match (actual, expected) {
        (Some(a), Some(e)) => Some(a - e),
        _ => None,
    };

    // If the WO hasn't been closed yet, compute live values from sub-entities
    let labor_cost_val: f64 = row
        .try_get::<f64>("", "labor_cost")
        .map_err(|e| decode_err("labor_cost", e))?;
    let parts_cost_val: f64 = row
        .try_get::<f64>("", "parts_cost")
        .map_err(|e| decode_err("parts_cost", e))?;
    let service_cost_val: f64 = row
        .try_get::<f64>("", "service_cost")
        .map_err(|e| decode_err("service_cost", e))?;
    let total_cost_val: f64 = row
        .try_get::<f64>("", "total_cost")
        .map_err(|e| decode_err("total_cost", e))?;

    // If total_cost is 0 and we have sub-entity data, compute live
    let (labor, parts, service, total) = if total_cost_val == 0.0 {
        let live = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT \
                    COALESCE((SELECT SUM(hours_worked * COALESCE(hourly_rate, 0)) \
                              FROM work_order_interveners WHERE work_order_id = ?), 0) AS live_labor, \
                    COALESCE((SELECT SUM(quantity_used * COALESCE(unit_cost, 0)) \
                              FROM work_order_parts WHERE work_order_id = ?), 0) AS live_parts",
                [wo_id.into(), wo_id.into()],
            ))
            .await?;

        if let Some(lr) = live {
            let ll: f64 = lr.try_get::<f64>("", "live_labor").unwrap_or(0.0);
            let lp: f64 = lr.try_get::<f64>("", "live_parts").unwrap_or(0.0);
            let ls = service_cost_val;
            (ll, lp, ls, ll + lp + ls)
        } else {
            (labor_cost_val, parts_cost_val, service_cost_val, total_cost_val)
        }
    } else {
        (labor_cost_val, parts_cost_val, service_cost_val, total_cost_val)
    };

    Ok(WoCostSummary {
        wo_id,
        labor_cost: labor,
        parts_cost: parts,
        service_cost: service,
        total_cost: total,
        expected_duration_hours: expected,
        actual_duration_hours: actual,
        active_labor_hours: row
            .try_get::<f64>("", "active_labor_hours")
            .map_err(|e| decode_err("active_labor_hours", e))?,
        total_waiting_hours: row
            .try_get::<f64>("", "total_waiting_hours")
            .map_err(|e| decode_err("total_waiting_hours", e))?,
        duration_variance_hours: variance,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_cost_posting_hook — payload for SP §6.24
// ═══════════════════════════════════════════════════════════════════════════════

/// Assemble the cost-posting hook payload for budget module consumption.
/// WO must be in `closed` or `technically_verified` state.
pub async fn get_cost_posting_hook(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<CostPostingHook> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                wo.id AS wo_id, \
                wo.code AS wo_code, \
                wo.entity_id, \
                wo.equipment_id AS asset_id, \
                wot.code AS type_code, \
                ul.level AS urgency_level, \
                COALESCE(wo.total_cost, 0) AS total_cost, \
                COALESCE(wo.labor_cost, 0) AS labor_cost, \
                COALESCE(wo.parts_cost, 0) AS parts_cost, \
                COALESCE(wo.service_cost, 0) AS service_cost, \
                wo.closed_at, \
                wos.code AS status_code \
             FROM work_orders wo \
             JOIN work_order_types    wot ON wot.id = wo.type_id \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             LEFT JOIN urgency_levels  ul ON ul.id  = wo.urgency_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    let status_code: String = row
        .try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;

    if !matches!(status_code.as_str(), "closed" | "technically_verified") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le hook de cout n'est disponible que pour les OT clotures ou verifies techniquement. \
             Statut actuel : '{status_code}'."
        )]));
    }

    Ok(CostPostingHook {
        wo_id: row
            .try_get::<i64>("", "wo_id")
            .map_err(|e| decode_err("wo_id", e))?,
        wo_code: row
            .try_get::<String>("", "wo_code")
            .map_err(|e| decode_err("wo_code", e))?,
        entity_id: row
            .try_get::<Option<i64>>("", "entity_id")
            .map_err(|e| decode_err("entity_id", e))?,
        asset_id: row
            .try_get::<Option<i64>>("", "asset_id")
            .map_err(|e| decode_err("asset_id", e))?,
        type_code: row
            .try_get::<String>("", "type_code")
            .map_err(|e| decode_err("type_code", e))?,
        urgency_level: row
            .try_get::<Option<i64>>("", "urgency_level")
            .map_err(|e| decode_err("urgency_level", e))?,
        total_cost: row
            .try_get::<f64>("", "total_cost")
            .map_err(|e| decode_err("total_cost", e))?,
        labor_cost: row
            .try_get::<f64>("", "labor_cost")
            .map_err(|e| decode_err("labor_cost", e))?,
        parts_cost: row
            .try_get::<f64>("", "parts_cost")
            .map_err(|e| decode_err("parts_cost", e))?,
        service_cost: row
            .try_get::<f64>("", "service_cost")
            .map_err(|e| decode_err("service_cost", e))?,
        closed_at: row
            .try_get::<Option<String>>("", "closed_at")
            .map_err(|e| decode_err("closed_at", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) update_service_cost
// ═══════════════════════════════════════════════════════════════════════════════

/// Update the manual service cost input on a WO.
/// WO must not be in closed or cancelled state.
pub async fn update_service_cost(
    db: &DatabaseConnection,
    wo_id: i64,
    service_cost: f64,
    _actor_id: i64,
) -> AppResult<()> {
    // Guard: load status
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

    let status_code: String = row
        .try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;

    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier le cout de service pour un OT en statut '{status_code}'."
        )]));
    }

    if service_cost < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Le cout de service ne peut pas etre negatif.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET service_cost_input = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE id = ?",
        [service_cost.into(), wo_id.into()],
    ))
    .await?;

    Ok(())
}
