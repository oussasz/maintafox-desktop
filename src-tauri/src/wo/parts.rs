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
//!   - Only the stock right (`inv.manage`) moves stock (L3-21, CMMS Advisor option A). A person
//!     without it who adds or records the use of a stock article records the line WITHOUT a stock
//!     movement: the quantity still to leave the store is `pending_issue_qty` ("à sortir"), the
//!     stock right holders are told, and the storekeeper issues it from the issue desk (confirming
//!     or correcting the quantity, audited) or cancels it with a reason (audited). A work order is
//!     not closed while a line has a quantity "à sortir" (`wo::closeout`). Free-text lines carry no
//!     stock and are unchanged.

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{InventoryIssueInput, InventoryReleaseReservationInput, InventoryReserveInput};
use crate::inventory::queries as inventory_queries;
use crate::wo::execution_log::{emit_execution_event, part_label_json};
use crate::wo::workflow::state_machine::status_i18n_key;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
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
    /// Used quantity of a stock article not yet issued from the store ("à sortir"): recorded by
    /// someone without the stock right, issued or cancelled by the storekeeper. 0 otherwise.
    #[serde(default)]
    pub pending_issue_qty: f64,
}

/// Two quantities closer than this are the same (sums of decimal quantities in floating point).
const QTY_EPSILON: f64 = 1e-9;

/// A stock line whose issued quantity has no trace in the store: no reservation and no location.
/// Every stock issue of a line goes through a reservation (and an issue from the parts "à sortir"
/// also records its location), so such a figure was only the mirror of the use that versions
/// before L3-21 wrote on a line without a location: nothing left the store for it.
fn issued_without_store_trace(part: &WoPart) -> bool {
    part.reservation_id.is_none() && part.stock_location_id.is_none()
}

/// Whether the person acting may move stock (`inv.manage`, L3-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockAuthority {
    /// Holds the stock right (or the system acting for a seeded or generated record): the used
    /// quantity of a stock article leaves the store at once.
    MayMoveStock,
    /// No stock right: the used quantity of a stock article is recorded "à sortir" for the
    /// storekeeper, with no stock movement.
    RequestOnly,
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
        pending_issue_qty: row
            .try_get::<f64>("", "pending_issue_qty")
            .map_err(|e| decode_err("pending_issue_qty", e))?,
    })
}

/// Load the WO status code.
async fn load_wo_status_code(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<String> {
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
     nur.label AS not_used_reason_label, wop.is_required, wop.pending_issue_qty, ",
    crate::reference::units::unit_symbol_of!("a.unit_value_id"),
    " AS unit_symbol"
);

const PART_FROM: &str = "\
    work_order_parts wop \
    LEFT JOIN articles a ON a.id = wop.article_id \
    LEFT JOIN reference_values nur ON nur.id = wop.not_used_reason_id";

/// Reserve then issue `quantity` of an execution-added stock article for its WO and
/// record the reservation on the part line, in one transaction: a failed issue leaves neither
/// the reservation nor the movement.
async fn reserve_and_issue_execution_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    part: &WoPart,
    article_id: i64,
    location_id: i64,
    quantity: f64,
) -> AppResult<WoPart> {
    let tx = db.begin().await?;
    let issued = reserve_and_issue_execution_part_in(&tx, part, article_id, location_id, quantity).await?;
    tx.commit().await?;
    Ok(issued)
}

/// [`reserve_and_issue_execution_part`] inside the caller's transaction.
async fn reserve_and_issue_execution_part_in<C: ConnectionTrait>(
    db: &C,
    part: &WoPart,
    article_id: i64,
    location_id: i64,
    quantity: f64,
) -> AppResult<WoPart> {
    let (source_type, _source_di_id, wo_code) = load_wo_source_context(db, part.work_order_id).await?;
    let reservation_id = inventory_queries::reserve_stock_in(
        db,
        &InventoryReserveInput {
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
    inventory_queries::issue_reserved_stock_in(
        db,
        &InventoryIssueInput {
            reservation_id,
            quantity,
            source_type: Some(source_type),
            source_id: Some(part.work_order_id),
            source_ref: Some(wo_code),
            notes: Some("WO execution part issue".to_string()),
        },
    )
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts \
         SET reservation_id = ?, quantity_reserved = ?, quantity_issued = ? \
         WHERE id = ?",
        [reservation_id.into(), quantity.into(), quantity.into(), part.id.into()],
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

async fn load_wo_source_context(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<(String, Option<i64>, String)> {
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

/// Inside a caller's transaction (PM content of a generated work order) the reservation's own
/// transaction is a savepoint of the caller's. A stock article added as used during execution
/// leaves the store at once with [`StockAuthority::MayMoveStock`]; with
/// [`StockAuthority::RequestOnly`] it is recorded "à sortir" with no stock movement (module
/// documentation). A planned line only reserves, whoever adds it.
pub async fn add_planned_part<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    input: AddPartInput,
    authority: StockAuthority,
) -> AppResult<WoPart> {
    let status_code = load_wo_status_code(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        let status_key = status_i18n_key(&status_code);
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_WO_PART_WO_LOCKED:status={status_key}: Impossible d'ajouter des pièces à un OT {status_code}."
        )]));
    }

    if input.quantity_planned < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_PART_PLANNED_NEGATIVE: quantity_planned doit être >= 0.".to_string(),
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
            "GATE_WO_ORIGIN_INVALID: origin invalide (planned|execution_added).".into(),
        ]));
    }
    // During execution only execution_added may be created (plan is frozen).
    if matches!(status_code.as_str(), "in_progress" | "on_hold" | "completed" | "ready")
        && origin == "planned"
        && matches!(status_code.as_str(), "in_progress" | "on_hold" | "completed")
    {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_PART_PLANNED_FROZEN: Impossible d'ajouter une pièce planifiée pendant l'exécution — utilisez une pièce ajoutée.".into(),
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
    // Without the stock right a stock article used during execution is "à sortir": no movement,
    // whatever `auto_reserve` says, so no stock article is ever used unaccounted.
    let pending_issue_qty = if authority == StockAuthority::RequestOnly
        && origin == "execution_added"
        && input.article_id.is_some()
        && input.quantity_planned > 0.0
    {
        input.quantity_planned
    } else {
        0.0
    };

    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_parts \
         (work_order_id, article_id, article_ref, quantity_planned, quantity_used, unit_cost, \
          stock_location_id, notes, origin, consumption_status, is_required, pending_issue_qty) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                pending_issue_qty.into(),
            ],
        ))
        .await?;

    let part_rowid = crate::db::inserted_id(&inserted)?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.rowid = ?"),
            [part_rowid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read part after insert")))?;
    let mut created = map_part(&row)?;

    // A stock article added during execution is taken from stock at once: reserve and
    // issue the used quantity so the movement, on-hand figure and WO parts cost agree.
    // Free-text (non-stock) lines carry no article and skip this. If stock cannot be
    // reserved or issued, the line is removed so no unissued "used" part remains. A line
    // "à sortir" (no stock right) waits for the storekeeper instead.
    let issue_execution_part = origin == "execution_added" && input.auto_reserve.unwrap_or(true);
    if pending_issue_qty > 0.0 {
        notify_part_to_issue(db, &created).await;
    } else if issue_execution_part {
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
                "GATE_WO_PART_LOCATION_REQUIRED: Choisissez l'emplacement de stock d'où la pièce est sortie.".into(),
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

/// Records the used quantity of a part line. The stock issue (or return) of the difference and
/// the line's used quantity are written in one transaction: a failure leaves neither, so a retry
/// never issues the parts twice. With [`StockAuthority::RequestOnly`] nothing leaves the store:
/// the used quantity above what was issued is "à sortir" (module documentation), and a used
/// quantity below what was issued is refused (the storekeeper records the return).
pub async fn record_actual_usage(
    db: &DatabaseConnection,
    wo_part_id: i64,
    quantity_used: f64,
    unit_cost: Option<f64>,
    authority: StockAuthority,
) -> AppResult<WoPart> {
    let tx = db.begin().await?;
    let before = load_part_in(&tx, wo_part_id).await?;
    let part = record_actual_usage_in(&tx, wo_part_id, quantity_used, unit_cost, authority).await?;
    tx.commit().await?;
    if part.pending_issue_qty > before.pending_issue_qty {
        notify_part_to_issue(db, &part).await;
    }
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

async fn record_actual_usage_in<C: ConnectionTrait>(
    db: &C,
    wo_part_id: i64,
    quantity_used: f64,
    unit_cost: Option<f64>,
    authority: StockAuthority,
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
        let status_key = status_i18n_key(&status_code);
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_WO_PART_USAGE_STATUS:status={status_key}: Les réels des pièces ne peuvent être saisis qu'au statut 'in_progress', \
             'on_hold' ou 'completed'. Statut actuel : '{status_code}'."
        )]));
    }

    if quantity_used < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_PART_USED_NEGATIVE: quantity_used doit être >= 0.".to_string(),
        ]));
    }

    // What already left the store for this line; the used quantity above it is issued now (stock
    // right) or "à sortir" (no stock right, or no stock location to issue from).
    // A figure without a store trace (written before L3-21 on a line without a location) left
    // nothing: it counts as 0, so the whole use is "à sortir" and nothing is refused as a return.
    let previous_issued = if part.article_id.is_some() && issued_without_store_trace(&part) {
        0.0
    } else {
        part.quantity_issued.max(0.0)
    };
    // The same quantity typed again (0.3 against an issued 0.30000000000000004) is no change.
    let quantity_used = if (quantity_used - previous_issued).abs() <= QTY_EPSILON {
        previous_issued
    } else {
        quantity_used
    };
    let return_required = || {
        AppError::ValidationFailed(vec![
            "GATE_WO_PART_RETURN_REQUIRED: Cette pièce a déjà été sortie du stock : enregistrez un retour magasin pour réduire la quantité."
                .to_string(),
        ])
    };
    let (new_issued, new_pending) = if part.article_id.is_none() {
        // Free text: no stock behind the line.
        (quantity_used, 0.0)
    } else if authority == StockAuthority::RequestOnly {
        if quantity_used < previous_issued {
            return Err(return_required());
        }
        (previous_issued, quantity_used - previous_issued)
    } else if let Some(reservation_id) = part.reservation_id {
        if quantity_used > previous_issued {
            let delta = quantity_used - previous_issued;
            let (source_type, _source_di_id, wo_code) = load_wo_source_context(db, part.work_order_id).await?;
            inventory_queries::issue_reserved_stock_in(
                db,
                &InventoryIssueInput {
                    reservation_id,
                    quantity: delta,
                    source_type: Some(source_type),
                    source_id: Some(part.work_order_id),
                    source_ref: Some(wo_code),
                    notes: Some("WO part usage issue".to_string()),
                },
            )
            .await?;
        } else if quantity_used < previous_issued {
            inventory_queries::return_reserved_stock_in(
                db,
                &crate::inventory::domain::InventoryReturnInput {
                    reservation_id,
                    quantity: previous_issued - quantity_used,
                    notes: Some("WO part usage rollback".to_string()),
                },
                None,
            )
            .await?;
        }
        (quantity_used, 0.0)
    } else if let (Some(article_id), Some(location_id)) = (part.article_id, part.stock_location_id) {
        // Stock article with no reservation (planned without stock, or added before stock was
        // chosen): the used quantity still leaves the store. Reserve and issue the increase so
        // on-hand, the stock movement and the WO parts cost agree.
        if quantity_used > previous_issued {
            reserve_and_issue_execution_part_in(db, &part, article_id, location_id, quantity_used - previous_issued)
                .await?;
        } else if quantity_used < previous_issued {
            return Err(return_required());
        }
        (quantity_used, 0.0)
    } else {
        // Stock article with no stock location: nothing to issue from here, the storekeeper
        // issues it from the location the part is taken from ("à sortir").
        if quantity_used < previous_issued {
            return Err(return_required());
        }
        (previous_issued, quantity_used - previous_issued)
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_parts SET \
            quantity_used = ?, \
            quantity_issued = ?, \
            pending_issue_qty = ?, \
            unit_cost = COALESCE(?, unit_cost), \
            consumption_status = 'used', \
            not_used_reason_id = NULL, \
            not_used_comment = NULL \
         WHERE id = ?",
        [
            quantity_used.into(),
            new_issued.into(),
            new_pending.into(),
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
    map_part(&updated)
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
        let status_key = status_i18n_key(&status_code);
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_WO_PART_NOT_USED_STATUS:status={status_key}: Impossible de marquer une pièce non utilisée au statut '{status_code}'."
        )]));
    }

    if part.origin != "planned" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_PART_NOT_USED_PLANNED_ONLY: Seules les pièces planifiées peuvent être marquées non utilisées."
                .into(),
        ]));
    }
    // Stock that left the store for this line goes back through a store return first; a quantity
    // only "à sortir" never left and is simply dropped with the line's use.
    let store_issued = match part.reservation_id {
        Some(reservation_id) => reservation_issued_quantity(db, reservation_id).await?,
        None => 0.0,
    };
    let line_issued = !issued_without_store_trace(&part) && part.quantity_issued > 0.0;
    if part.article_id.is_some() && (line_issued || store_issued > 0.0) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_PART_RETURN_REQUIRED: Cette pièce a déjà été sortie du stock : enregistrez un retour magasin pour réduire la quantité."
                .to_string(),
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
                "GATE_WO_PART_REASON_UNKNOWN:reason_id={0}: Motif de non-utilisation introuvable (id={0}).",
                input.not_used_reason_id
            )])
        })?;
    let reason_code: String = reason_row.try_get("", "code").map_err(|e| decode_err("code", e))?;
    let comment = input.not_used_comment.as_ref().map(|s| s.trim().to_string());
    if reason_code.eq_ignore_ascii_case("other") && comment.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_PART_REASON_COMMENT: Un commentaire est obligatoire lorsque le motif est « Other ».".into(),
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
            pending_issue_qty = 0, \
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
            "GATE_WO_PART_PLANNED_PENDING: Des pièces planifiées existent : marquez chaque ligne Utilisée ou Non utilisée. \
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

// ═══════════════════════════════════════════════════════════════════════════════
// F) Parts "à sortir" — issued or cancelled by the storekeeper (L3-21)
// ═══════════════════════════════════════════════════════════════════════════════

/// Notification category of a part line "à sortir" (migration 275): the stock right holders.
pub const PART_TO_ISSUE_CATEGORY: &str = "wo_part_to_issue";

/// Where the notice leads: the issue desk of the stock screen.
pub const PARTS_TO_ISSUE_URL: &str = "/inventory?stock=to_issue";

/// Least length of the reason of a correction or a cancellation of a quantity "à sortir".
pub const TO_ISSUE_REASON_MIN_CHARS: usize = 5;

/// Stable codes of the refusals on a part line "à sortir" (translated in `training:errors`).
pub mod to_issue_codes {
    pub const NOTHING_TO_ISSUE: &str = "WO_PART_NOTHING_TO_ISSUE";
    pub const QUANTITY_INVALID: &str = "WO_PART_TO_ISSUE_QUANTITY_INVALID";
    pub const REASON_REQUIRED: &str = "WO_PART_TO_ISSUE_REASON_REQUIRED";
    pub const LOCATION_REQUIRED: &str = "WO_PART_TO_ISSUE_LOCATION_REQUIRED";
    pub const WO_LOCKED: &str = "WO_PART_TO_ISSUE_WO_LOCKED";
}

/// A part line "à sortir" with its work order, for the storekeeper's list.
#[derive(Debug, Clone, Serialize)]
pub struct PartToIssue {
    #[serde(flatten)]
    pub part: WoPart,
    pub wo_code: String,
    pub wo_title: Option<String>,
    pub wo_status_code: String,
    /// `warehouse/location` codes of the line's stock location, when it has one.
    pub location_label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssuePartToIssueInput {
    pub wo_part_id: i64,
    /// The quantity the storekeeper issues: the quantity "à sortir", or a correction of it.
    pub quantity: f64,
    /// The location the part leaves from (a line without a reservation; the line's own location
    /// otherwise). A line with a reservation issues from its reservation's location.
    pub stock_location_id: Option<i64>,
    /// Why the issued quantity differs from the quantity "à sortir" (required then).
    pub correction_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CancelPartToIssueInput {
    pub wo_part_id: i64,
    pub reason: String,
}

/// The line, still "à sortir", of a work order not closed or cancelled.
async fn load_line_to_issue<C: ConnectionTrait>(conn: &C, wo_part_id: i64) -> AppResult<WoPart> {
    let part = load_part_in(conn, wo_part_id).await?;
    refuse_if_cancelled(&part)?;
    if part.pending_issue_qty <= 0.0 || part.article_id.is_none() {
        return Err(part_line_refusal(
            to_issue_codes::NOTHING_TO_ISSUE,
            "This part line has nothing left to issue: reload the list.",
            &[],
        ));
    }
    Ok(part)
}

fn trimmed_reason(reason: Option<&str>) -> Option<String> {
    reason
        .map(str::trim)
        .filter(|r| r.chars().count() >= TO_ISSUE_REASON_MIN_CHARS)
        .map(str::to_string)
}

/// Tell the stock right holders (at the work order's site) that a part line is "à sortir".
/// Fire-and-log: the line is recorded whether or not the notice could be raised.
async fn notify_part_to_issue(db: &impl ConnectionTrait, part: &WoPart) {
    let wo_code = match load_wo_source_context(db, part.work_order_id).await {
        Ok((_, _, code)) => code,
        Err(err) => {
            tracing::warn!(error = %err, wo_part_id = part.id, "part to issue notice not raised");
            return;
        }
    };
    let label = part_display_label(part);
    let quantity = part.pending_issue_qty;
    let unit = part.unit_symbol.clone().unwrap_or_default();
    let input = crate::notifications::emitter::NotificationEventInput {
        source_module: "wo".into(),
        source_record_id: Some(part.work_order_id.to_string()),
        event_code: PART_TO_ISSUE_CATEGORY.into(),
        category_code: PART_TO_ISSUE_CATEGORY.into(),
        severity: "info".into(),
        dedupe_key: None,
        payload_json: None,
        title: format!("À sortir pour {wo_code} : {label}"),
        body: Some(format!(
            "{quantity} {unit} de {label} à sortir du magasin pour l'OT {wo_code}."
        )),
        action_url: Some(PARTS_TO_ISSUE_URL.to_string()),
    };
    let message = crate::notifications::emitter::NotificationMessage::new(
        PART_TO_ISSUE_CATEGORY,
        serde_json::json!({ "wo": wo_code, "part": label, "quantity": quantity, "unit": unit }),
    );
    if let Err(err) = crate::notifications::emitter::emit_event_with(db, input, Some(&message)).await {
        tracing::warn!(error = %err, wo_part_id = part.id, "part to issue notice not raised");
    }
}

/// The part lines "à sortir" of the work orders not cancelled, oldest work order first.
pub async fn list_parts_to_issue(db: &DatabaseConnection) -> AppResult<Vec<PartToIssue>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT {PART_COLS}, wo.code AS wo_code, wo.title AS wo_title, wos.code AS wo_status_code, \
                        w.code || '/' || sl.code AS location_label \
                 FROM {PART_FROM} \
                 JOIN work_orders wo ON wo.id = wop.work_order_id \
                 JOIN work_order_statuses wos ON wos.id = wo.status_id \
                 LEFT JOIN stock_locations sl ON sl.id = wop.stock_location_id \
                 LEFT JOIN warehouses w ON w.id = sl.warehouse_id \
                 WHERE wop.pending_issue_qty > 0 AND wop.consumption_status <> 'cancelled' \
                   AND wos.code <> 'cancelled' \
                 ORDER BY wo.id ASC, wop.id ASC"
            ),
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(PartToIssue {
                part: map_part(row)?,
                wo_code: row.try_get("", "wo_code").map_err(|e| decode_err("wo_code", e))?,
                wo_title: row.try_get("", "wo_title").map_err(|e| decode_err("wo_title", e))?,
                wo_status_code: row
                    .try_get("", "wo_status_code")
                    .map_err(|e| decode_err("wo_status_code", e))?,
                location_label: row
                    .try_get("", "location_label")
                    .map_err(|e| decode_err("location_label", e))?,
            })
        })
        .collect()
}

/// The storekeeper issues a part line "à sortir": in one transaction the stock leaves the store
/// (from the line's reservation, or reserved and issued at the chosen location), the line's
/// issued and used quantities become what was issued, nothing stays "à sortir", and the issue is
/// audited (requested and issued quantities, reason of a correction) with a WO history event.
/// The stock right is checked by the command.
pub async fn issue_part_to_issue(
    db: &DatabaseConnection,
    input: IssuePartToIssueInput,
    actor_id: i64,
) -> AppResult<WoPart> {
    let txn = db.begin().await?;
    let part = load_line_to_issue(&txn, input.wo_part_id).await?;
    let status_code = load_wo_status_code(&txn, part.work_order_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(part_line_refusal(
            to_issue_codes::WO_LOCKED,
            format!("The work order is {status_code}: nothing can be issued to it."),
            &[("status", status_i18n_key(&status_code).to_string())],
        ));
    }
    if !input.quantity.is_finite() || input.quantity <= 0.0 {
        return Err(part_line_refusal(
            to_issue_codes::QUANTITY_INVALID,
            "The issued quantity must be greater than zero; cancel the line to issue nothing.",
            &[],
        ));
    }
    let requested = part.pending_issue_qty;
    let corrected = (input.quantity - requested).abs() > QTY_EPSILON;
    let reason = trimmed_reason(input.correction_reason.as_deref());
    if corrected && reason.is_none() {
        return Err(part_line_refusal(
            to_issue_codes::REASON_REQUIRED,
            format!("Give the reason why the issued quantity differs from the requested {requested}."),
            &[("min", TO_ISSUE_REASON_MIN_CHARS.to_string())],
        ));
    }
    let article_id = part.article_id.unwrap_or_default();

    let issued_from = if let Some(reservation_id) = part.reservation_id {
        // A use above the planned quantity: what the reservation still holds is issued from it,
        // the rest is reserved and issued at the same location (the line keeps its reservation).
        let held = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT location_id, status, quantity_reserved - COALESCE(quantity_issued, 0.0) AS remaining \
                 FROM stock_reservations WHERE id = ?",
                [reservation_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "StockReservation".into(),
                id: reservation_id.to_string(),
            })?;
        let location_id: i64 = held
            .try_get("", "location_id")
            .map_err(|e| decode_err("location_id", e))?;
        let status: String = held.try_get("", "status").map_err(|e| decode_err("status", e))?;
        let remaining: f64 = held.try_get("", "remaining").map_err(|e| decode_err("remaining", e))?;
        let from_reservation = if status == "released" {
            0.0
        } else {
            input.quantity.min(remaining.max(0.0))
        };
        let excess = input.quantity - from_reservation;
        let (source_type, _source_di_id, wo_code) = load_wo_source_context(&txn, part.work_order_id).await?;
        if from_reservation > QTY_EPSILON {
            inventory_queries::issue_reserved_stock_in(
                &txn,
                &InventoryIssueInput {
                    reservation_id,
                    quantity: from_reservation,
                    source_type: Some(source_type.clone()),
                    source_id: Some(part.work_order_id),
                    source_ref: Some(wo_code.clone()),
                    notes: Some("WO part issued from the parts to issue".to_string()),
                },
            )
            .await?;
        }
        if excess > QTY_EPSILON {
            let extra = inventory_queries::reserve_stock_in(
                &txn,
                &InventoryReserveInput {
                    article_id,
                    location_id,
                    quantity: excess,
                    source_type: source_type.clone(),
                    source_id: Some(part.work_order_id),
                    source_ref: Some(wo_code.clone()),
                    notes: Some("WO part used above its reservation".to_string()),
                },
            )
            .await?;
            inventory_queries::issue_reserved_stock_in(
                &txn,
                &InventoryIssueInput {
                    reservation_id: extra,
                    quantity: excess,
                    source_type: Some(source_type),
                    source_id: Some(part.work_order_id),
                    source_ref: Some(wo_code),
                    notes: Some("WO part issued from the parts to issue".to_string()),
                },
            )
            .await?;
        }
        Some(location_id)
    } else {
        let Some(location_id) = input.stock_location_id.or(part.stock_location_id) else {
            return Err(part_line_refusal(
                to_issue_codes::LOCATION_REQUIRED,
                "Choose the stock location the part leaves from.",
                &[],
            ));
        };
        reserve_and_issue_execution_part_in(&txn, &part, article_id, location_id, input.quantity).await?;
        Some(location_id)
    };

    let issued_before = if issued_without_store_trace(&part) {
        0.0
    } else {
        part.quantity_issued.max(0.0)
    };
    let issued_total = issued_before + input.quantity;
    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_parts SET quantity_issued = ?, quantity_used = ?, pending_issue_qty = 0, \
                stock_location_id = COALESCE(stock_location_id, ?), consumption_status = 'used' \
         WHERE id = ? AND pending_issue_qty = ?",
            [
                issued_total.into(),
                issued_total.into(),
                issued_from
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                part.id.into(),
                requested.into(),
            ],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        // Changed by someone else between the read and the write: nothing is kept.
        return Err(part_line_refusal(
            part_line_codes::CHANGED,
            "This part line was changed meanwhile: reload the list.",
            &[],
        ));
    }

    let label = part_display_label(&part);
    crate::wo::audit::insert_wo_change_event(
        &txn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(part.work_order_id),
            action: "part_to_issue_issued".into(),
            actor_id: Some(actor_id),
            summary: Some(if corrected {
                format!(
                    "Part issued by the store: {label} x {} (requested {requested})",
                    input.quantity
                )
            } else {
                format!("Part issued by the store: {label} x {}", input.quantity)
            }),
            details_json: Some(
                serde_json::json!({
                    "wo_part_id": part.id,
                    "article_id": part.article_id,
                    "label": label,
                    "unit_symbol": part.unit_symbol,
                    "quantity_requested": requested,
                    "quantity_issued": input.quantity,
                    "corrected": corrected,
                    "correction_reason": reason,
                    "stock_location_id": issued_from,
                    "reservation_id": part.reservation_id,
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
        "part_issued",
        "executionLog.partIssued",
        part_label_json(&label),
        Some("part"),
        Some(part.id),
        Some(actor_id),
    )
    .await?;
    let issued = load_part_in(&txn, part.id).await?;
    txn.commit().await?;
    Ok(issued)
}

/// The quantity "à sortir" of a line is withdrawn with a reason (nothing left the store for it):
/// the line's used quantity goes back to what was issued; a line with nothing issued goes back to
/// planned (a planned line) or is cancelled (a line added during execution). Audited with the
/// reason, with a WO history event, in one transaction. The command checks the rights (the stock
/// right, or the WO edit right on the work order).
pub async fn cancel_part_to_issue(
    db: &DatabaseConnection,
    input: CancelPartToIssueInput,
    actor_id: i64,
) -> AppResult<WoPart> {
    let txn = db.begin().await?;
    let part = load_line_to_issue(&txn, input.wo_part_id).await?;
    let Some(reason) = trimmed_reason(Some(&input.reason)) else {
        return Err(part_line_refusal(
            to_issue_codes::REASON_REQUIRED,
            "Give the reason why this part is not issued.",
            &[("min", TO_ISSUE_REASON_MIN_CHARS.to_string())],
        ));
    };
    let issued = part.quantity_issued.max(0.0);
    let (status, used): (&str, Option<f64>) = if issued > 0.0 {
        ("used", Some(issued))
    } else if part.origin == "planned" {
        ("pending", None)
    } else {
        (PART_STATUS_CANCELLED, Some(0.0))
    };
    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_parts SET pending_issue_qty = 0, quantity_used = ?, consumption_status = ? \
             WHERE id = ? AND pending_issue_qty = ?",
            [
                used.map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                status.into(),
                part.id.into(),
                part.pending_issue_qty.into(),
            ],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(part_line_refusal(
            part_line_codes::CHANGED,
            "This part line was changed meanwhile: reload the list.",
            &[],
        ));
    }

    let label = part_display_label(&part);
    crate::wo::audit::insert_wo_change_event(
        &txn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(part.work_order_id),
            action: "part_to_issue_cancelled".into(),
            actor_id: Some(actor_id),
            summary: Some(format!(
                "Part not issued: {label} x {} ({reason})",
                part.pending_issue_qty
            )),
            details_json: Some(
                serde_json::json!({
                    "wo_part_id": part.id,
                    "article_id": part.article_id,
                    "label": label,
                    "unit_symbol": part.unit_symbol,
                    "quantity_withdrawn": part.pending_issue_qty,
                    "quantity_used_before": part.quantity_used,
                    "quantity_issued": issued,
                    "line_status_after": status,
                    "reason": reason,
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
        "part_to_issue_cancelled",
        "executionLog.partToIssueCancelled",
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
                    COALESCE(quantity_issued, 0.0) AS issued, pending_issue_qty, reservation_id \
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
    let local_pending: f64 = local
        .try_get("", "pending_issue_qty")
        .map_err(|e| decode_err("pending_issue_qty", e))?;
    // A use that is entirely "à sortir" never left the store: the storekeeper's cancellation of
    // it (`cancel_part_to_issue`) withdraws the line without any stock to account for.
    let only_to_issue = local_pending > 0.0 && local_issued <= 0.0 && local_used <= local_pending;
    if local_issued > 0.0 || (!only_to_issue && (status == "used" || local_used > 0.0)) {
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
