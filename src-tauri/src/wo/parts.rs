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
    /// `pending` | `used` | `not_used`
    #[serde(default = "default_consumption_status")]
    pub consumption_status: String,
    pub not_used_reason_id: Option<i64>,
    pub not_used_comment: Option<String>,
    #[serde(default)]
    pub not_used_reason_label: Option<String>,
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

const PART_COLS: &str = "wop.id, wop.work_order_id, wop.article_id, wop.article_ref, wop.quantity_planned, \
     wop.quantity_used, wop.unit_cost, wop.stock_location_id, wop.reservation_id, \
     wop.quantity_reserved, wop.quantity_issued, wop.notes, \
     COALESCE(NULLIF(TRIM(wop.article_ref), ''), a.article_code) AS article_label, \
     wop.origin, wop.consumption_status, wop.not_used_reason_id, wop.not_used_comment, \
     nur.label AS not_used_reason_label";

const PART_FROM: &str = "\
    work_order_parts wop \
    LEFT JOIN articles a ON a.id = wop.article_id \
    LEFT JOIN reference_values nur ON nur.id = wop.not_used_reason_id";

async fn load_wo_source_context(db: &DatabaseConnection, wo_id: i64) -> AppResult<(String, Option<i64>, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.code AS wo_code, wo.source_di_id, wot.code AS type_code
             FROM work_orders wo
             JOIN work_order_types wot ON wot.id = wo.type_id
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
    let type_code: String = row.try_get("", "type_code").map_err(|e| decode_err("type_code", e))?;
    let source_type = if type_code.eq_ignore_ascii_case("preventive") {
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
          stock_location_id, notes, origin, consumption_status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.rowid = last_insert_rowid()"),
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read part after insert")))?;
    let mut created = map_part(&row)?;

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
                        &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
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
            &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: wo_part_id.to_string(),
        })?;
    let part = map_part(&part_row)?;

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
            &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
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
            &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
            [input.wo_part_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoPart".into(),
            id: input.wo_part_id.to_string(),
        })?;
    let part = map_part(&part_row)?;

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
            &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.id = ?"),
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
             WHERE work_order_id = ? AND COALESCE(origin, 'planned') = 'planned'",
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
            &format!("SELECT {PART_COLS} FROM {PART_FROM} WHERE wop.work_order_id = ? ORDER BY wop.id ASC"),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_part).collect()
}
