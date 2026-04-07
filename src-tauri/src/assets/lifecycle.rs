//! Asset lifecycle event service.
//!
//! Phase 2 - Sub-phase 02 - File 02 - Sprint S1.
//!
//! Provides governed lifecycle history for assets on top of the existing
//! `equipment_lifecycle_events` table (migration 005, extended by
//! migration 011 with classification, replacement, reason, and approval columns).
//!
//! Column reconciliation (roadmap → DB):
//!   roadmap field        → DB column
//!   ─────────────────────────────────────────
//!   asset_id             → equipment_lifecycle_events.equipment_id
//!   event_type           → equipment_lifecycle_events.event_type
//!   event_at             → equipment_lifecycle_events.occurred_at
//!   from_org_node_id     → equipment_lifecycle_events.from_node_id
//!   to_org_node_id       → equipment_lifecycle_events.to_node_id
//!   from_status_code     → equipment_lifecycle_events.from_status
//!   to_status_code       → equipment_lifecycle_events.to_status
//!   from_class_code      → equipment_lifecycle_events.from_class_code  (migration 011)
//!   to_class_code        → equipment_lifecycle_events.to_class_code    (migration 011)
//!   related_asset_id     → equipment_lifecycle_events.related_asset_id (migration 011)
//!   reason_code          → equipment_lifecycle_events.reason_code      (migration 011)
//!   approved_by_id       → equipment_lifecycle_events.approved_by_id   (migration 011)
//!   created_by_id        → equipment_lifecycle_events.performed_by_id
//!   created_at           → equipment_lifecycle_events.created_at
//!
//! Lifecycle events are append-only. They are never updated or deleted.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Read-side lifecycle event record. Fields use roadmap naming for the
/// IPC boundary; column reconciliation is handled in `map_lifecycle_event`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetLifecycleEvent {
    pub id: i64,
    pub sync_id: String,
    pub asset_id: i64,
    pub event_type: String,
    pub event_at: String,
    pub from_org_node_id: Option<i64>,
    pub to_org_node_id: Option<i64>,
    pub from_status_code: Option<String>,
    pub to_status_code: Option<String>,
    pub from_class_code: Option<String>,
    pub to_class_code: Option<String>,
    pub related_asset_id: Option<i64>,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
    pub approved_by_id: Option<i64>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

/// Payload for recording a lifecycle event. The caller supplies event-type
/// and the relevant context fields; the service validates required
/// combinations per event type.
#[derive(Debug, Deserialize)]
pub struct RecordLifecycleEventPayload {
    pub asset_id: i64,
    pub event_type: String,
    pub event_at: Option<String>,
    pub from_org_node_id: Option<i64>,
    pub to_org_node_id: Option<i64>,
    pub from_status_code: Option<String>,
    pub to_status_code: Option<String>,
    pub from_class_code: Option<String>,
    pub to_class_code: Option<String>,
    pub related_asset_id: Option<i64>,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
    pub approved_by_id: Option<i64>,
}

// ─── Constants ────────────────────────────────────────────────────────────────

/// Event types that produce side-effects on the equipment registry row.
const STATUS_CHANGING_EVENTS: &[&str] = &["DECOMMISSIONED", "RECOMMISSIONED"];

// ─── Row mapping ──────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "equipment_lifecycle_events row decode failed for column '{column}': {e}"
    ))
}

fn map_lifecycle_event(row: &QueryResult) -> AppResult<AssetLifecycleEvent> {
    Ok(AssetLifecycleEvent {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        sync_id: row
            .try_get::<String>("", "sync_id")
            .map_err(|e| decode_err("sync_id", e))?,
        asset_id: row
            .try_get::<i64>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        event_type: row
            .try_get::<String>("", "event_type")
            .map_err(|e| decode_err("event_type", e))?,
        event_at: row
            .try_get::<String>("", "occurred_at")
            .map_err(|e| decode_err("occurred_at", e))?,
        from_org_node_id: row
            .try_get::<Option<i64>>("", "from_node_id")
            .map_err(|e| decode_err("from_node_id", e))?,
        to_org_node_id: row
            .try_get::<Option<i64>>("", "to_node_id")
            .map_err(|e| decode_err("to_node_id", e))?,
        from_status_code: row
            .try_get::<Option<String>>("", "from_status")
            .map_err(|e| decode_err("from_status", e))?,
        to_status_code: row
            .try_get::<Option<String>>("", "to_status")
            .map_err(|e| decode_err("to_status", e))?,
        from_class_code: row
            .try_get::<Option<String>>("", "from_class_code")
            .map_err(|e| decode_err("from_class_code", e))?,
        to_class_code: row
            .try_get::<Option<String>>("", "to_class_code")
            .map_err(|e| decode_err("to_class_code", e))?,
        related_asset_id: row
            .try_get::<Option<i64>>("", "related_asset_id")
            .map_err(|e| decode_err("related_asset_id", e))?,
        reason_code: row
            .try_get::<Option<String>>("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        approved_by_id: row
            .try_get::<Option<i64>>("", "approved_by_id")
            .map_err(|e| decode_err("approved_by_id", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "performed_by_id")
            .map_err(|e| decode_err("performed_by_id", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

// ─── Validation helpers ───────────────────────────────────────────────────────

/// Validate that `event_type` exists in the `equipment.lifecycle_event_type`
/// lookup domain.
async fn validate_event_type(
    db: &impl ConnectionTrait,
    code: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'equipment.lifecycle_event_type' \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type d'evenement '{code}' introuvable dans le domaine \
             'equipment.lifecycle_event_type'."
        )]));
    }
    Ok(())
}

/// Assert that an equipment row exists and is not soft-deleted.
/// Returns `(lifecycle_status, installed_at_node_id, class_id)` for
/// pre/post-event state capture.
async fn assert_asset_exists(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<(String, Option<i64>, Option<i64>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lifecycle_status, installed_at_node_id, class_id \
             FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    let status: String = row
        .try_get("", "lifecycle_status")
        .map_err(|e| decode_err("lifecycle_status", e))?;
    let node_id: Option<i64> = row
        .try_get("", "installed_at_node_id")
        .map_err(|e| decode_err("installed_at_node_id", e))?;
    let class_id: Option<i64> = row
        .try_get("", "class_id")
        .map_err(|e| decode_err("class_id", e))?;
    Ok((status, node_id, class_id))
}

/// Validate that the related asset exists and is not soft-deleted.
async fn assert_related_asset_exists(
    db: &impl ConnectionTrait,
    related_asset_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [related_asset_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::NotFound {
            entity: "equipment (related_asset)".into(),
            id: related_asset_id.to_string(),
        });
    }
    Ok(())
}

/// Validate event-type-specific payload requirements.
fn validate_payload_for_event_type(payload: &RecordLifecycleEventPayload) -> AppResult<()> {
    let et = payload.event_type.as_str();
    let mut errors: Vec<String> = Vec::new();

    match et {
        "MOVED" => {
            if payload.from_org_node_id.is_none() {
                errors.push(
                    "Un evenement de type 'MOVED' requiert 'from_org_node_id'.".into(),
                );
            }
            if payload.to_org_node_id.is_none() {
                errors.push(
                    "Un evenement de type 'MOVED' requiert 'to_org_node_id'.".into(),
                );
            }
        }
        "REPLACED" => {
            if payload.related_asset_id.is_none() {
                errors.push(
                    "Un evenement de type 'REPLACED' requiert 'related_asset_id'.".into(),
                );
            }
        }
        "RECLASSIFIED" => {
            if payload.from_class_code.is_none() {
                errors.push(
                    "Un evenement de type 'RECLASSIFIED' requiert 'from_class_code'.".into(),
                );
            }
            if payload.to_class_code.is_none() {
                errors.push(
                    "Un evenement de type 'RECLASSIFIED' requiert 'to_class_code'.".into(),
                );
            }
        }
        // Other event types have no mandatory extra fields beyond the base set.
        _ => {}
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    Ok(())
}

// ─── Service functions ────────────────────────────────────────────────────────

/// SELECT columns for lifecycle event queries.
const EVENT_SELECT: &str = r"
    id, sync_id, equipment_id, event_type, occurred_at,
    from_node_id, to_node_id, from_status, to_status,
    from_class_code, to_class_code, related_asset_id,
    reason_code, notes, approved_by_id, performed_by_id, created_at
";

/// List lifecycle events for an asset, ordered newest-first.
///
/// # Arguments
/// - `asset_id` — the equipment id
/// - `limit` — max rows (capped at 500)
pub async fn list_asset_lifecycle_events(
    db: &DatabaseConnection,
    asset_id: i64,
    limit: Option<u64>,
) -> AppResult<Vec<AssetLifecycleEvent>> {
    let row_limit = limit.unwrap_or(100).min(500);
    let sql = format!(
        "SELECT {EVENT_SELECT} \
         FROM equipment_lifecycle_events \
         WHERE equipment_id = ? \
         ORDER BY occurred_at DESC, id DESC \
         LIMIT {row_limit}"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [asset_id.into()],
        ))
        .await?;

    rows.iter().map(map_lifecycle_event).collect()
}

/// Record a governed lifecycle event.
///
/// Validation:
///   - `event_type` must exist in `equipment.lifecycle_event_type` domain
///   - `asset_id` must exist and not be soft-deleted
///   - move events require both org-node ids
///   - replacement events require `related_asset_id`
///   - reclassification events require both class codes
///
/// Side-effects:
///   - `DECOMMISSIONED` → sets `equipment.lifecycle_status = 'DECOMMISSIONED'`
///     and `equipment.decommissioned_at`
///   - `RECOMMISSIONED` → sets `equipment.lifecycle_status` to
///     `payload.to_status_code` (required) and clears `decommissioned_at`
pub async fn record_lifecycle_event(
    db: &DatabaseConnection,
    mut payload: RecordLifecycleEventPayload,
    actor_id: i32,
) -> AppResult<AssetLifecycleEvent> {
    let txn = db.begin().await?;

    // ── 1. Validate event type against lookup domain ─────────────────────
    validate_event_type(&txn, &payload.event_type).await?;

    // ── 2. Validate asset exists ─────────────────────────────────────────
    let (current_status, current_node_id, _current_class_id) =
        assert_asset_exists(&txn, payload.asset_id).await?;

    // ── 2b. Auto-enrich payload from current asset state ─────────────────
    // For MOVED events, capture the current org node as from_org_node_id
    // if the caller did not supply it explicitly.
    if payload.event_type == "MOVED" && payload.from_org_node_id.is_none() {
        payload.from_org_node_id = current_node_id;
    }
    // For status-changing events, capture the current status as from_status.
    if STATUS_CHANGING_EVENTS.contains(&payload.event_type.as_str())
        && payload.from_status_code.is_none()
    {
        payload.from_status_code = Some(current_status.clone());
    }

    // ── 3. Validate event-type-specific payload requirements ─────────────
    validate_payload_for_event_type(&payload)?;

    // ── 4. Validate related asset if provided ────────────────────────────
    if let Some(related_id) = payload.related_asset_id {
        assert_related_asset_exists(&txn, related_id).await?;
    }

    // ── 5. Apply side-effects for status-changing events ─────────────────
    let now = Utc::now().to_rfc3339();
    let event_at = payload.event_at.clone().unwrap_or_else(|| now.clone());

    match payload.event_type.as_str() {
        "DECOMMISSIONED" => {
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED', \
                 decommissioned_at = ?, updated_at = ?, \
                 row_version = row_version + 1 \
                 WHERE id = ?",
                [
                    event_at.clone().into(),
                    now.clone().into(),
                    payload.asset_id.into(),
                ],
            ))
            .await?;
        }
        "RECOMMISSIONED" => {
            let target_status = payload.to_status_code.as_deref().unwrap_or("ACTIVE_IN_SERVICE");
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE equipment SET lifecycle_status = ?, \
                 decommissioned_at = NULL, updated_at = ?, \
                 row_version = row_version + 1 \
                 WHERE id = ?",
                [
                    target_status.into(),
                    now.clone().into(),
                    payload.asset_id.into(),
                ],
            ))
            .await?;
        }
        "MOVED" => {
            // Update installed_at_node_id to reflect the move.
            if let Some(to_node_id) = payload.to_org_node_id {
                txn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE equipment SET installed_at_node_id = ?, \
                     updated_at = ?, row_version = row_version + 1 \
                     WHERE id = ?",
                    [
                        to_node_id.into(),
                        now.clone().into(),
                        payload.asset_id.into(),
                    ],
                ))
                .await?;
            }
        }
        _ => {}
    }

    // ── 6. Insert the lifecycle event row ────────────────────────────────
    let sync_id = Uuid::new_v4().to_string();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_lifecycle_events \
         (sync_id, equipment_id, event_type, occurred_at, \
          from_node_id, to_node_id, from_status, to_status, \
          from_class_code, to_class_code, related_asset_id, \
          reason_code, notes, approved_by_id, performed_by_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            sync_id.clone().into(),
            payload.asset_id.into(),
            payload.event_type.into(),
            event_at.into(),
            payload.from_org_node_id.into(),
            payload.to_org_node_id.into(),
            payload.from_status_code.into(),
            payload.to_status_code.into(),
            payload.from_class_code.into(),
            payload.to_class_code.into(),
            payload.related_asset_id.into(),
            payload.reason_code.into(),
            payload.notes.into(),
            payload.approved_by_id.into(),
            (actor_id as i64).into(),
            now.into(),
        ],
    ))
    .await?;

    // ── 7. Retrieve the inserted event ───────────────────────────────────
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {EVENT_SELECT} \
                 FROM equipment_lifecycle_events WHERE sync_id = ?"
            ),
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "lifecycle event created but not found after insert"
            ))
        })?;
    let event = map_lifecycle_event(&row)?;

    txn.commit().await?;

    tracing::info!(
        event_id = event.id,
        asset_id = event.asset_id,
        event_type = %event.event_type,
        "lifecycle event recorded"
    );
    Ok(event)
}
