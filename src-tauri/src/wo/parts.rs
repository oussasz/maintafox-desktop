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
//!   - A planned line is never deleted. While the WO is prepared (draft, planning) a line with
//!     no recorded use can be cancelled (`consumption_status = 'cancelled'`): its reservation
//!     is released and an audit entry written in the same transaction. A cancelled line leaves
//!     every count, cost, readiness and shortage figure and stays in the WO history; adding the
//!     part again makes a new line with its own reservation. Required / optional can be
//!     switched on the same lines, audited the same way.

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{InventoryIssueInput, InventoryReleaseReservationInput, InventoryReserveInput};
use crate::inventory::queries as inventory_queries;
use crate::wo::execution_log::{emit_execution_event, part_label_json};
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
    pub reservation_id: Option<i64>,
    pub quantity_reserved: f64,
    pub quantity_issued: f64,
    pub notes: Option<String>,
    /// Prefer `article_ref`, else catalog article_code from JOIN.
    #[serde(default)]
    pub article_label: Option<String>,
    /// `planned` | `execution_added`
    #[serde(default = "default_part_origin")]
    pub origin: String,
    /// `pending` | `used` | `not_used` | `cancelled` (planned line withdrawn before execution)
    #[serde(default = "default_consumption_status")]
    pub consumption_status: String,
    pub not_used_reason_id: Option<i64>,
    pub not_used_comment: Option<String>,
    #[serde(default)]
    pub not_used_reason_label: Option<String>,
    /// Required for the job: short, it blocks the plan; optional: short, it only warns.
    pub is_required: bool,
    /// Stock unit symbol of the article (unit list of Données de référence); `None` for a
    /// free-text line.
    #[serde(default)]
    pub unit_symbol: Option<String>,
}

fn default_part_origin() -> String {
    "planned".into()
}
fn default_consumption_status() -> String {
    "pending".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddPartInput {
    pub wo_id: i64,
    pub article_id: Option<i64>,
    pub article_ref: Option<String>,
    pub quantity_planned: f64,
    pub unit_cost: Option<f64>,
    pub stock_location_id: Option<i64>,
    pub auto_reserve: Option<bool>,
    pub notes: Option<String>,
    /// Force origin; otherwise derived from WO status.
    pub origin: Option<String>,
    /// Required for the job (a short required part blocks the plan) or optional (a short optional
    /// part warns). No default: every new line says which.
    pub is_required: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarkPartNotUsedInput {
    pub wo_part_id: i64,
    pub not_used_reason_id: i64,
    pub not_used_comment: Option<String>,
    pub actor_id: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WoPart row decode error for '{field}': {e}"))
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
        reservation_id: row
            .try_get::<Option<i64>>("", "reservation_id")
            .map_err(|e| decode_err("reservation_id", e))?,
        quantity_reserved: row
            .try_get::<f64>("", "quantity_reserved")
            .map_err(|e| decode_err("quantity_reserved", e))?,
        quantity_issued: row
            .try_get::<f64>("", "quantity_issued")
            .map_err(|e| decode_err("quantity_issued", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        article_label: row.try_get::<Option<String>>("", "article_label").unwrap_or(None),
        origin: row
            .try_get::<Option<String>>("", "origin")
            .ok()
            .flatten()
            .unwrap_or_else(|| "planned".into()),
        consumption_status: row
            .try_get::<Option<String>>("", "consumption_status")
            .ok()
            .flatten()
            .unwrap_or_else(|| "pending".into()),
        not_used_reason_id: row.try_get::<Option<i64>>("", "not_used_reason_id").unwrap_or(None),
        not_used_comment: row.try_get::<Option<String>>("", "not_used_comment").unwrap_or(None),
        not_used_reason_label: row
            .try_get::<Option<String>>("", "not_used_reason_label")
            .unwrap_or(None),
        is_required: row
            .try_get::<i64>("", "is_required")
            .map_err(|e| decode_err("is_required", e))?
            == 1,
        unit_symbol: row
            .try_get::<Option<String>>("", "unit_symbol")
            .map_err(|e| decode_err("unit_symbol", e))?,
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

const PART_COLS: &str = concat!(
    "wop.id, wop.work_order_id, wop.article_id, wop.article_ref, wop.quantity_planned, \
     wop.quantity_used, wop.unit_cost, wop.stock_location_id, wop.reservation_id, \
     wop.quantity_reserved, wop.quantity_issued, wop.notes, \
     COALESCE(NULLIF(TRIM(wop.article_ref), ''), a.article_code) AS article_label, \
     wop.origin, wop.consumption_status, wop.not_used_reason_id, wop.not_used_comment, \
     nur.label AS not_used_reason_label, wop.is_required, ",
    crate::reference::units::unit_symbol_of!("a.unit_value_id"),
    " AS unit_symbol"
);

const PART_FROM: &str = "\
    work_order_parts wop \
    LEFT JOIN articles a ON a.id = wop.article_id \
    LEFT JOIN reference_values nur ON nur.id = wop.not_used_reason_id";

/// Reserve then issue `quantity` of an execution-added stock article for its WO and
/// record the reservation on the part line. A failed issue releases the reservation.
async fn reserve_and_issue_execution_part(
    db: &DatabaseConnection,
    part: &WoPart,
    article_id: i64,
    location_id: i64,
    quantity: f64,
) -> AppResult<WoPart> {
    let (source_type, _source_di_id, wo_code) = load_wo_source_context(db, part.work_order_id).await?;
    let reservation = inventory_queries::reserve_stock(
        db,
        InventoryReserveInput {
            article_id,
            location_id,
            quantity,
            source_type: source_type.clone(),
            source_id: Some(part.work_order_id),
            source_ref: Some(wo_code.clone()),
            notes: Some("WO execution part reservation".to_string()),
        },
    )
    .await?;

    if let Err(err) = inventory_queries::issue_reserved_stock(
        db,
        InventoryIssueInput {
            reservation_id: reservation.id,
            quantity,
            source_type: Some(source_type),
            source_id: Some(part.work_order_id),
            source_ref: Some(wo_code),
            notes: Some("WO execution part issue".to_string()),
        },
    )
    .await
    {
        let _ = inventory_queries::release_stock_reservation(
            db,
            InventoryReleaseReservationInput {
                reservation_id: reservation.id,
                notes: Some("WO execution part issue failed".to_string()),
            },
        )
        .await;
        return Err(err);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts \
         SET reservation_id = ?, quantity_reserved = ?, quantity_issued = ? \
         WHERE id = ?",
        [reservation.id.into(), quantity.into(), quantity.into(), part.id.into()],
    ))
    .await?;

    // Carry the stock valuation onto the line when no cost was entered, so the WO parts cost
    // reflects what left the store. Without a valuation policy the cost stays empty (visible in
    // the cost summary) rather than being invented.
    if part.unit_cost.is_none() {
        let warehouse_id: Option<i64> = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT warehouse_id FROM stock_locations WHERE id = ?",
                [location_id.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<i64>("", "warehouse_id").ok());
        if let Some(warehouse_id) = warehouse_id {
            if let Ok(valuation) =
                crate::inventory::valuation::evaluate_unit_cost(db, article_id, warehouse_id, location_id).await
            {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE work_order_parts SET unit_cost = COALESCE(unit_cost, ?) WHERE id = ?",
                    [valuation.unit_cost.into(), part.id.into()],
                ))
                .await?;
            }
        }
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [part.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read part after issue")))?;
    map_part(&row)
}

async fn load_wo_source_context(db: &DatabaseConnection, wo_id: i64) -> AppResult<(String, Option<i64>, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.code AS wo_code, wo.source_di_id, wot.semantic_tag AS nature_code
             FROM work_orders wo
             LEFT JOIN reference_values wot ON wot.id = wo.type_id
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let wo_code: String = row.try_get("", "wo_code").map_err(|e| decode_err("wo_code", e))?;
    let source_di_id: Option<i64> = row
        .try_get("", "source_di_id")
        .map_err(|e| decode_err("source_di_id", e))?;
    let nature_code: Option<String> = row
        .try_get("", "nature_code")
        .map_err(|e| decode_err("nature_code", e))?;
    // Parts consumed by preventive work (any preventive nature, EN 13306) are PM demand.
    let is_preventive = nature_code
        .as_deref()
        .and_then(crate::wo::types::MaintenanceNature::parse)
        .is_some_and(|n| n.is_preventive());
    let source_type = if is_preventive {
        "PM_WO".to_string()
    } else {
        "WO".to_string()
    };
    Ok((source_type, source_di_id, wo_code))
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) add_planned_part
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn add_planned_part(db: &DatabaseConnection, input: AddPartInput) -> AppResult<WoPart> {
    let status_code = load_wo_status_code(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible d'ajouter des pièces à un OT {status_code}."
        )]));
    }

    if input.quantity_planned < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity_planned doit être >= 0.".to_string()
        ]));
    }

    let origin = if let Some(ref o) = input.origin {
        o.clone()
    } else if matches!(status_code.as_str(), "in_progress" | "on_hold") {
        "execution_added".to_string()
    } else {
        "planned".to_string()
    };
    if !matches!(origin.as_str(), "planned" | "execution_added") {
        return Err(AppError::ValidationFailed(vec![
            "origin invalide (planned|execution_added).".into(),
        ]));
    }
    // During execution only execution_added may be created (plan is frozen).
    if matches!(status_code.as_str(), "in_progress" | "on_hold" | "completed" | "ready")
        && origin == "planned"
        && matches!(status_code.as_str(), "in_progress" | "on_hold" | "completed")
    {
        return Err(AppError::ValidationFailed(vec![
            "Impossible d'ajouter une pièce planifiée pendant l'exécution — utilisez une pièce ajoutée.".into(),
        ]));
    }

    let qty_for_insert = if origin == "execution_added" {
        // Execution-added lines carry planned qty 0; usage recorded separately or as used.
        0.0
    } else {
        input.quantity_planned
    };
    let initial_status = if origin == "execution_added" && input.quantity_planned > 0.0 {
        "used"
    } else {
        "pending"
    };
    let initial_used = if initial_status == "used" {
        Some(input.quantity_planned)
    } else {
        None
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_parts \
         (work_order_id, article_id, article_ref, quantity_planned, quantity_used, unit_cost, \
          stock_location_id, notes, origin, consumption_status, is_required) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            if origin == "execution_added" {
                input.quantity_planned.into()
            } else {
                qty_for_insert.into()
            },
            initial_used
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            input
                .unit_cost
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            input
                .stock_location_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            input
                .notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            origin.clone().into(),
            initial_status.into(),
            i64::from(input.is_required).into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.rowid = last_insert_rowid()"),
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read part after insert")))?;
    let mut created = map_part(&row)?;

    // A stock article added during execution is taken from stock at once: reserve and
    // issue the used quantity so the movement, on-hand figure and WO parts cost agree.
    // Free-text (non-stock) lines carry no article and skip this. If stock cannot be
    // reserved or issued, the line is removed so no unissued "used" part remains.
    let issue_execution_part = origin == "execution_added" && input.auto_reserve.unwrap_or(true);
    if issue_execution_part {
        if let (Some(article_id), Some(location_id)) = (created.article_id, created.stock_location_id) {
            if input.quantity_planned > 0.0 {
                match reserve_and_issue_execution_part(db, &created, article_id, location_id, input.quantity_planned)
                    .await
                {
                    Ok(updated) => created = updated,
                    Err(err) => {
                        db.execute(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "DELETE FROM work_order_parts WHERE id = ?",
                            [created.id.into()],
                        ))
                        .await?;
                        return Err(err);
                    }
                }
            }
        } else if input.article_id.is_some() {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM work_order_parts WHERE id = ?",
                [created.id.into()],
            ))
            .await?;
            return Err(AppError::ValidationFailed(vec![
                "Choisissez l'emplacement de stock d'où la pièce est sortie.".into(),
            ]));
        }
    }

    let should_auto_reserve = input.auto_reserve.unwrap_or(true) && origin == "planned";
    if should_auto_reserve {
        if let (Some(article_id), Some(location_id)) = (created.article_id, created.stock_location_id) {
            if created.quantity_planned > 0.0 {
                let (source_type, _source_di_id, wo_code) = load_wo_source_context(db, created.work_order_id).await?;
                let reservation = inventory_queries::reserve_stock(
                    db,
                    InventoryReserveInput {
                        article_id,
                        location_id,
                        quantity: created.quantity_planned,
                        source_type,
                        source_id: Some(created.work_order_id),
                        source_ref: Some(wo_code),
                        notes: Some("WO planned part reservation".to_string()),
                    },
                )
                .await?;
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE work_order_parts
                     SET reservation_id = ?, quantity_reserved = ?
                     WHERE id = ?",
                    [
                        reservation.id.into(),
                        reservation.quantity_reserved.into(),
                        created.id.into(),
                    ],
                ))
                .await?;
                let updated = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
                        [created.id.into()],
                    ))
                    .await?
                    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read part after reservation")))?;
                created = map_part(&updated)?;
            }
        }
    }

    let label = created
        .article_label
        .clone()
        .or(created.article_ref.clone())
        .unwrap_or_else(|| format!("#{}", created.id));
    let _ = emit_execution_event(
        db,
        created.work_order_id,
        if origin == "execution_added" {
            "part_added"
        } else {
            "part_planned"
        },
        if origin == "execution_added" {
            "executionLog.partAdded"
        } else {
            "executionLog.partPlanned"
        },
        part_label_json(&label),
        Some("part"),
        Some(created.id),
        None,
    )
    .await;

    Ok(created)
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
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: wo_part_id.to_string(),
        })?;
    let part = map_part(&part_row)?;
    refuse_if_cancelled(&part)?;

    let status_code = load_wo_status_code(db, part.work_order_id).await?;
    if !matches!(status_code.as_str(), "in_progress" | "on_hold" | "completed") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Les réels des pièces ne peuvent être saisis qu'au statut 'in_progress', \
             'on_hold' ou 'completed'. Statut actuel : '{status_code}'."
        )]));
    }

    if quantity_used < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity_used doit être >= 0.".to_string()
        ]));
    }

    if let Some(reservation_id) = part.reservation_id {
        let previous_used = part.quantity_used.unwrap_or(0.0);
        if quantity_used > previous_used {
            let delta = quantity_used - previous_used;
            let (source_type, _source_di_id, wo_code) = load_wo_source_context(db, part.work_order_id).await?;
            inventory_queries::issue_reserved_stock(
                db,
                InventoryIssueInput {
                    reservation_id,
                    quantity: delta,
                    source_type: Some(source_type),
                    source_id: Some(part.work_order_id),
                    source_ref: Some(wo_code),
                    notes: Some("WO part usage issue".to_string()),
                },
            )
            .await?;
        } else if quantity_used < previous_used {
            inventory_queries::return_reserved_stock(
                db,
                crate::inventory::domain::InventoryReturnInput {
                    reservation_id,
                    quantity: previous_used - quantity_used,
                    notes: Some("WO part usage rollback".to_string()),
                },
            )
            .await?;
        }
    } else if let (Some(article_id), Some(location_id)) = (part.article_id, part.stock_location_id) {
        // Stock article with no reservation (planned without stock, or added before stock was
        // chosen): the used quantity still leaves the store. Reserve and issue the increase so
        // on-hand, the stock movement and the WO parts cost agree.
        let previous_issued = part.quantity_issued.max(0.0);
        if quantity_used > previous_issued {
            let issued =
                reserve_and_issue_execution_part(db, &part, article_id, location_id, quantity_used - previous_issued)
                    .await?;
            // `reserve_and_issue_execution_part` records the delta; keep the cumulative figure.
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_parts SET quantity_issued = ? WHERE id = ?",
                [quantity_used.into(), issued.id.into()],
            ))
            .await?;
        } else if quantity_used < previous_issued {
            return Err(AppError::ValidationFailed(vec![
                "Cette pièce a déjà été sortie du stock : enregistrez un retour magasin pour réduire la quantité."
                    .to_string(),
            ]));
        }
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts SET \
            quantity_used = ?, \
            quantity_issued = ?, \
            unit_cost = COALESCE(?, unit_cost), \
            consumption_status = 'used', \
            not_used_reason_id = NULL, \
            not_used_comment = NULL \
         WHERE id = ?",
        [
            quantity_used.into(),
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
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: wo_part_id.to_string(),
        })?;
    let part = map_part(&updated)?;
    let label = part
        .article_label
        .clone()
        .or(part.article_ref.clone())
        .unwrap_or_else(|| format!("#{}", part.id));
    let _ = emit_execution_event(
        db,
        part.work_order_id,
        "part_used",
        "executionLog.partUsed",
        part_label_json(&label),
        Some("part"),
        Some(part.id),
        None,
    )
    .await;
    Ok(part)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B2) mark_part_not_used
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn mark_part_not_used(db: &DatabaseConnection, input: MarkPartNotUsedInput) -> AppResult<WoPart> {
    let part_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [input.wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: input.wo_part_id.to_string(),
        })?;
    let part = map_part(&part_row)?;
    refuse_if_cancelled(&part)?;

    let status_code = load_wo_status_code(db, part.work_order_id).await?;
    if !matches!(status_code.as_str(), "in_progress" | "on_hold" | "completed") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de marquer une pièce non utilisée au statut '{status_code}'."
        )]));
    }

    if part.origin != "planned" {
        return Err(AppError::ValidationFailed(vec![
            "Seules les pièces planifiées peuvent être marquées non utilisées.".into(),
        ]));
    }

    // Validate reason is WORK.PART_UNUSED_REASON
    let reason_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code AS code, rv.label AS label \
             FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rv.id = ? AND rv.is_active = 1 AND rs.status = 'published' \
               AND UPPER(TRIM(rd.code)) = 'WORK.PART_UNUSED_REASON'",
            [input.not_used_reason_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "Motif de non-utilisation introuvable (id={}).",
                input.not_used_reason_id
            )])
        })?;
    let reason_code: String = reason_row.try_get("", "code").map_err(|e| decode_err("code", e))?;
    let comment = input.not_used_comment.as_ref().map(|s| s.trim().to_string());
    if reason_code.eq_ignore_ascii_case("other") && comment.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
        return Err(AppError::ValidationFailed(vec![
            "Un commentaire est obligatoire lorsque le motif est « Other ».".into(),
        ]));
    }

    if let Some(reservation_id) = part.reservation_id {
        let _ = inventory_queries::release_stock_reservation(
            db,
            InventoryReleaseReservationInput {
                reservation_id,
                notes: Some("WO planned part marked not used".to_string()),
            },
        )
        .await;
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts SET \
            consumption_status = 'not_used', \
            quantity_used = 0, \
            not_used_reason_id = ?, \
            not_used_comment = ?, \
            reservation_id = NULL, \
            quantity_reserved = 0 \
         WHERE id = ?",
        [
            input.not_used_reason_id.into(),
            comment
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input.wo_part_id.into(),
        ],
    ))
    .await?;

    let updated = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [input.wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: input.wo_part_id.to_string(),
        })?;
    let part = map_part(&updated)?;
    let label = part
        .article_label
        .clone()
        .or(part.article_ref.clone())
        .unwrap_or_else(|| format!("#{}", part.id));
    let _ = emit_execution_event(
        db,
        part.work_order_id,
        "part_not_used",
        "executionLog.partNotUsed",
        part_label_json(&label),
        Some("part"),
        Some(part.id),
        input.actor_id,
    )
    .await;
    Ok(part)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) confirm_no_parts_used
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn confirm_no_parts_used(db: &DatabaseConnection, wo_id: i64, actor_id: i64) -> AppResult<()> {
    // Only when there are no planned part lines — planned lines must be
    // disposed individually (used / not_used).
    let planned = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM work_order_parts \
             WHERE work_order_id = ? AND COALESCE(origin, 'planned') = 'planned' \
               AND consumption_status <> 'cancelled'",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("confirm_no_parts planned count returned no row")))?;
    let planned_count: i64 = planned.try_get("", "cnt").map_err(|e| decode_err("planned_count", e))?;
    if planned_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Des pièces planifiées existent : marquez chaque ligne Utilisée ou Non utilisée. \
(Planned parts exist: mark each line Used or Not used.)"
                .into(),
        ]));
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reservation_id FROM work_order_parts
             WHERE work_order_id = ? AND reservation_id IS NOT NULL",
            [wo_id.into()],
        ))
        .await?;
    for row in rows {
        let reservation_id: i64 = row
            .try_get("", "reservation_id")
            .map_err(|e| decode_err("reservation_id", e))?;
        inventory_queries::release_stock_reservation(
            db,
            InventoryReleaseReservationInput {
                reservation_id,
                notes: Some("WO marked as no parts used".to_string()),
            },
        )
        .await?;
    }

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

    let _ = emit_execution_event(
        db,
        wo_id,
        "parts_none_confirmed",
        "executionLog.partsNoneConfirmed",
        serde_json::json!({}),
        Some("parts"),
        None,
        Some(actor_id),
    )
    .await;

    Ok(())
}

/// Clear the "no parts used" attestation so the technician can add parts again.
pub async fn unconfirm_no_parts_used(db: &DatabaseConnection, wo_id: i64, actor_id: i64) -> AppResult<()> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET parts_actuals_confirmed = 0 WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        });
    }
    let _ = emit_execution_event(
        db,
        wo_id,
        "parts_none_cleared",
        "executionLog.partsNoneCleared",
        serde_json::json!({}),
        Some("parts"),
        None,
        Some(actor_id),
    )
    .await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) list_wo_parts
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_wo_parts(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoPart>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.work_order_id = ? ORDER BY wop.id ASC"),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_part).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) cancel_planned_part / set_planned_part_required
// ═══════════════════════════════════════════════════════════════════════════════

/// `consumption_status` of a planned line withdrawn before execution.
pub const PART_STATUS_CANCELLED: &str = "cancelled";

/// Stable codes of the refusals on a planned part line (translated in `training:errors`).
pub mod part_line_codes {
    pub const NOT_IN_PLANNING: &str = "WO_PART_LINE_NOT_IN_PLANNING";
    pub const NOT_PLANNED_LINE: &str = "WO_PART_LINE_NOT_PLANNED";
    pub const HAS_USE: &str = "WO_PART_LINE_HAS_USE";
    pub const SETTLED: &str = "WO_PART_LINE_SETTLED";
    pub const CANCELLED: &str = "WO_PART_LINE_CANCELLED";
    pub const CHANGED: &str = "WO_PART_LINE_CHANGED";
}

#[derive(Debug, Clone, Deserialize)]
pub struct CancelPlannedPartInput {
    pub wo_part_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetPlannedPartRequiredInput {
    pub wo_part_id: i64,
    pub is_required: bool,
}

fn part_line_refusal(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

/// A cancelled line takes no usage and no "not used" disposition.
fn refuse_if_cancelled(part: &WoPart) -> AppResult<()> {
    if part.consumption_status == PART_STATUS_CANCELLED {
        return Err(part_line_refusal(
            part_line_codes::CANCELLED,
            "This part line is cancelled.",
            &[],
        ));
    }
    Ok(())
}

async fn load_part_in<C: ConnectionTrait>(conn: &C, wo_part_id: i64) -> AppResult<WoPart> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: wo_part_id.to_string(),
        })?;
    map_part(&row)
}

/// The checks shared by cancel and required/optional (the WO edit right at the work order's site
/// is checked by the command, like the other part line actions): the WO is still prepared
/// (draft, planning), the line is a planned line, not cancelled, not settled and with no recorded
/// use (such a line keeps the "not used" action of execution).
async fn check_planned_line_editable<C: ConnectionTrait>(conn: &C, wo_part_id: i64) -> AppResult<WoPart> {
    use crate::wo::workflow::state_machine::WoStatus;
    let part = load_part_in(conn, wo_part_id).await?;
    let (_, status, _) = crate::wo::workflow::transition::load_wo_status(conn, part.work_order_id).await?;
    if !matches!(status, WoStatus::Draft | WoStatus::Planning) {
        return Err(part_line_refusal(
            part_line_codes::NOT_IN_PLANNING,
            format!(
                "Planned part lines can be changed while the work order is prepared (draft, planning); it is {}.",
                status.as_str()
            ),
            &[("status", status.as_str().to_string())],
        ));
    }
    if part.origin != "planned" {
        return Err(part_line_refusal(
            part_line_codes::NOT_PLANNED_LINE,
            "Only a planned part line can be changed here.",
            &[],
        ));
    }
    refuse_if_cancelled(&part)?;
    if part.consumption_status == "used" || part.quantity_used.unwrap_or(0.0) > 0.0 || part.quantity_issued > 0.0 {
        return Err(part_line_refusal(
            part_line_codes::HAS_USE,
            "This part line has recorded use: mark it not used instead.",
            &[],
        ));
    }
    if part.consumption_status != "pending" {
        return Err(part_line_refusal(
            part_line_codes::SETTLED,
            format!("This part line is already settled ({}).", part.consumption_status),
            &[("status", part.consumption_status.clone())],
        ));
    }
    Ok(part)
}

fn part_display_label(part: &WoPart) -> String {
    part.article_label
        .clone()
        .or(part.article_ref.clone())
        .unwrap_or_else(|| format!("#{}", part.id))
}

/// Cancel a planned part line (never deleted): in one transaction its reservation is released,
/// the line is marked `cancelled` and an audit entry (who, when, line, quantity) plus a WO history
/// event are written. Refused, with a coded reason, for a WO past planning, an execution-added
/// line, or a line with recorded use. The WO edit right at the work order's site is checked by
/// the command.
pub async fn cancel_planned_part(
    db: &DatabaseConnection,
    input: CancelPlannedPartInput,
    actor_id: i64,
) -> AppResult<WoPart> {
    use sea_orm::TransactionTrait;
    let txn = db.begin().await?;
    let part = check_planned_line_editable(&txn, input.wo_part_id).await?;

    if let Some(reservation_id) = part.reservation_id {
        // The store desk can issue against the WO reservation before execution (the line's own
        // issued figure is not updated then): stock that left the store is recorded use.
        let issued = reservation_issued_quantity(&txn, reservation_id).await?;
        if issued > 0.0 {
            return Err(part_line_refusal(
                part_line_codes::HAS_USE,
                format!(
                    "Stock was already issued from the store against this line ({issued}): mark it not used instead."
                ),
                &[],
            ));
        }
        // A release only lowers the reserved quantity: it works for an article deactivated since.
        inventory_queries::release_withdrawn_reservation_with_connection(
            &txn,
            reservation_id,
            Some("WO planned part line cancelled"),
        )
        .await?;
    }

    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_parts SET consumption_status = ?, reservation_id = NULL, quantity_reserved = 0 \
             WHERE id = ? AND consumption_status = 'pending'",
            [PART_STATUS_CANCELLED.into(), part.id.into()],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        // Changed by someone else between the read and the write: nothing is kept.
        return Err(part_line_refusal(
            part_line_codes::CHANGED,
            "This part line was changed meanwhile: reload the work order.",
            &[],
        ));
    }

    let label = part_display_label(&part);
    crate::wo::audit::insert_wo_change_event(
        &txn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(part.work_order_id),
            action: "part_line_cancelled".into(),
            actor_id: Some(actor_id),
            summary: Some(format!("Part line cancelled: {label} x {}", part.quantity_planned)),
            details_json: Some(
                serde_json::json!({
                    "wo_part_id": part.id,
                    "article_id": part.article_id,
                    "label": label,
                    "quantity_planned": part.quantity_planned,
                    "unit_symbol": part.unit_symbol,
                    "is_required": part.is_required,
                    "released_reservation_id": part.reservation_id,
                    "quantity_released": part.quantity_reserved,
                })
                .to_string(),
            ),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    emit_execution_event(
        &txn,
        part.work_order_id,
        "part_cancelled",
        "executionLog.partCancelled",
        part_label_json(&label),
        Some("part"),
        Some(part.id),
        Some(actor_id),
    )
    .await?;

    let cancelled = load_part_in(&txn, part.id).await?;
    txn.commit().await?;
    Ok(cancelled)
}

/// Switch a planned part line between required and optional while the WO is prepared. Same
/// refusals as [`cancel_planned_part`]; the change is audited (who, when, line, old -> new) in the
/// same transaction. Setting the value the line already has changes and records nothing.
pub async fn set_planned_part_required(
    db: &DatabaseConnection,
    input: SetPlannedPartRequiredInput,
    actor_id: i64,
) -> AppResult<WoPart> {
    use sea_orm::TransactionTrait;
    let txn = db.begin().await?;
    let part = check_planned_line_editable(&txn, input.wo_part_id).await?;
    if part.is_required == input.is_required {
        return Ok(part);
    }
    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_parts SET is_required = ? \
             WHERE id = ? AND consumption_status = 'pending' AND is_required = ?",
            [
                i64::from(input.is_required).into(),
                part.id.into(),
                i64::from(part.is_required).into(),
            ],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(part_line_refusal(
            part_line_codes::CHANGED,
            "This part line was changed meanwhile: reload the work order.",
            &[],
        ));
    }
    let label = part_display_label(&part);
    let necessity = |required: bool| if required { "required" } else { "optional" };
    crate::wo::audit::insert_wo_change_event(
        &txn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(part.work_order_id),
            action: "part_line_necessity_changed".into(),
            actor_id: Some(actor_id),
            summary: Some(format!(
                "Part line {label}: {} -> {}",
                necessity(part.is_required),
                necessity(input.is_required)
            )),
            details_json: Some(
                serde_json::json!({
                    "wo_part_id": part.id,
                    "article_id": part.article_id,
                    "label": label,
                    "quantity_planned": part.quantity_planned,
                    "from_required": part.is_required,
                    "to_required": input.is_required,
                })
                .to_string(),
            ),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    let (event_type, summary_key) = if input.is_required {
        ("part_set_required", "executionLog.partSetRequired")
    } else {
        ("part_set_optional", "executionLog.partSetOptional")
    };
    emit_execution_event(
        &txn,
        part.work_order_id,
        event_type,
        summary_key,
        part_label_json(&label),
        Some("part"),
        Some(part.id),
        Some(actor_id),
    )
    .await?;
    let changed = load_part_in(&txn, part.id).await?;
    txn.commit().await?;
    Ok(changed)
}

/// Quantity already issued from the store against a reservation (0 when the row is absent).
async fn reservation_issued_quantity<C: ConnectionTrait>(conn: &C, reservation_id: i64) -> AppResult<f64> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(quantity_issued, 0.0) AS issued FROM stock_reservations WHERE id = ?",
            [reservation_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<f64>("", "issued"))
        .transpose()
        .map_err(|e| decode_err("issued", e))?
        .unwrap_or(0.0))
}

/// Inbound sync check of a received part line (`sync::apply_hooks::before_upsert_checked`): a line
/// received as cancelled breaks the rule "only a line with no recorded use is cancelled" when
/// - the received row carries a used or issued quantity, or
/// - this device's copy of the line (found by its sync id) has recorded use: a used or issued
///   quantity, or stock issued from the store against its reservation.
///
/// Either is a conflict for a person to decide, never applied silently. A cancelled row carries no
/// reservation (cancelling clears it), so when this device has no copy of the line yet the store
/// issues cannot be determined on receipt: only the received quantities are checked then.
pub(crate) async fn inbound_part_line_check<C: ConnectionTrait>(
    conn: &C,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<crate::settings::site::InboundVerdict>> {
    use crate::settings::site::InboundVerdict;
    if payload.get("consumption_status").and_then(serde_json::Value::as_str) != Some(PART_STATUS_CANCELLED) {
        return Ok(None);
    }
    let conflict = |message: String| {
        Ok(Some(InboundVerdict::Conflict {
            code: part_line_codes::HAS_USE,
            message,
        }))
    };
    let qty = |key: &str| payload.get(key).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let (used, issued) = (qty("quantity_used"), qty("quantity_issued"));
    if used > 0.0 || issued > 0.0 {
        return conflict(format!(
            "a part line received as cancelled has recorded use (used {used}, issued {issued})"
        ));
    }
    let local = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT consumption_status, COALESCE(quantity_used, 0.0) AS used, \
                    COALESCE(quantity_issued, 0.0) AS issued, reservation_id \
               FROM work_order_parts WHERE entity_sync_id = ?",
            [entity_sync_id.into()],
        ))
        .await?;
    let Some(local) = local else {
        return Ok(None);
    };
    let status: String = local
        .try_get("", "consumption_status")
        .map_err(|e| decode_err("consumption_status", e))?;
    if status == PART_STATUS_CANCELLED {
        return Ok(None);
    }
    let local_used: f64 = local.try_get("", "used").map_err(|e| decode_err("used", e))?;
    let local_issued: f64 = local.try_get("", "issued").map_err(|e| decode_err("issued", e))?;
    if status == "used" || local_used > 0.0 || local_issued > 0.0 {
        return conflict(format!(
            "a part line received as cancelled has recorded use on this device (used {local_used}, issued {local_issued})"
        ));
    }
    let reservation_id: Option<i64> = local
        .try_get("", "reservation_id")
        .map_err(|e| decode_err("reservation_id", e))?;
    if let Some(reservation_id) = reservation_id {
        let store_issued = reservation_issued_quantity(conn, reservation_id).await?;
        if store_issued > 0.0 {
            return conflict(format!(
                "a part line received as cancelled had stock issued from the store against it on this device ({store_issued})"
            ));
        }
    }
    Ok(None)
}
