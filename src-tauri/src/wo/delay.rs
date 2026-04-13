//! WO delay and downtime segment management.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! Delay segments track time lost waiting (linked to a delay_reason_code) and are
//! created automatically by `pause_wo` / `set_waiting_for_prerequisite`. This module
//! exposes read-only list views and a manual downtime segment API (OEE / TEEP input).
//!
//! Downtime segment `downtime_type` enum:
//!   - `full`         — full production stop
//!   - `partial`      — reduced throughput
//!   - `standby`      — equipment idle, no production demand
//!   - `quality_loss` — running but producing rejects

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoDelaySegment {
    pub id: i64,
    pub work_order_id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub delay_reason_id: Option<i64>,
    pub comment: Option<String>,
    pub entered_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoDowntimeSegment {
    pub id: i64,
    pub work_order_id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub downtime_type: String,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenDowntimeInput {
    pub wo_id: i64,
    pub downtime_type: String,
    pub comment: Option<String>,
    pub actor_id: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "Delay/Downtime row decode error for '{field}': {e}"
    ))
}

fn map_delay_segment(row: &sea_orm::QueryResult) -> AppResult<WoDelaySegment> {
    Ok(WoDelaySegment {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        started_at: row
            .try_get::<String>("", "started_at")
            .map_err(|e| decode_err("started_at", e))?,
        ended_at: row
            .try_get::<Option<String>>("", "ended_at")
            .map_err(|e| decode_err("ended_at", e))?,
        delay_reason_id: row
            .try_get::<Option<i64>>("", "delay_reason_id")
            .map_err(|e| decode_err("delay_reason_id", e))?,
        comment: row
            .try_get::<Option<String>>("", "comment")
            .map_err(|e| decode_err("comment", e))?,
        entered_by_id: row
            .try_get::<Option<i64>>("", "entered_by_id")
            .map_err(|e| decode_err("entered_by_id", e))?,
    })
}

fn map_downtime_segment(row: &sea_orm::QueryResult) -> AppResult<WoDowntimeSegment> {
    Ok(WoDowntimeSegment {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        started_at: row
            .try_get::<String>("", "started_at")
            .map_err(|e| decode_err("started_at", e))?,
        ended_at: row
            .try_get::<Option<String>>("", "ended_at")
            .map_err(|e| decode_err("ended_at", e))?,
        downtime_type: row
            .try_get::<String>("", "downtime_type")
            .map_err(|e| decode_err("downtime_type", e))?,
        comment: row
            .try_get::<Option<String>>("", "comment")
            .map_err(|e| decode_err("comment", e))?,
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

const VALID_DOWNTIME_TYPES: &[&str] = &["full", "partial", "standby", "quality_loss"];

const DELAY_COLS: &str =
    "id, work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id";

const DOWNTIME_COLS: &str =
    "id, work_order_id, started_at, ended_at, downtime_type, comment";

// ═══════════════════════════════════════════════════════════════════════════════
// A) open_downtime_segment
// ═══════════════════════════════════════════════════════════════════════════════

/// Manually open a downtime segment (OEE input).
/// WO must be in [in_progress, paused, assigned].
pub async fn open_downtime_segment(
    db: &DatabaseConnection,
    input: OpenDowntimeInput,
) -> AppResult<WoDowntimeSegment> {
    if !VALID_DOWNTIME_TYPES.contains(&input.downtime_type.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "downtime_type invalide : '{}'. Valeurs autorisées : full, partial, standby, quality_loss.",
            input.downtime_type
        )]));
    }

    let status_code = load_wo_status_code(db, input.wo_id).await?;
    if !matches!(
        status_code.as_str(),
        "in_progress" | "paused" | "assigned"
    ) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Un segment de temps d'arrêt ne peut être ouvert qu'aux statuts \
             in_progress, paused ou assigned. Statut actuel : '{status_code}'."
        )]));
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_downtime_segments \
         (work_order_id, started_at, downtime_type, comment) \
         VALUES (?, ?, ?, ?)",
        [
            input.wo_id.into(),
            now.into(),
            input.downtime_type.into(),
            input
                .comment
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {DOWNTIME_COLS} FROM work_order_downtime_segments \
                 WHERE rowid = last_insert_rowid()"
            ),
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-read downtime segment after insert"
            ))
        })?;
    map_downtime_segment(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) close_downtime_segment
// ═══════════════════════════════════════════════════════════════════════════════

/// Close an open downtime segment (set ended_at).
pub async fn close_downtime_segment(
    db: &DatabaseConnection,
    segment_id: i64,
    ended_at: Option<String>,
) -> AppResult<WoDowntimeSegment> {
    let close_ts = ended_at
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_downtime_segments SET ended_at = ? \
             WHERE id = ? AND ended_at IS NULL",
            [close_ts.into(), segment_id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        // Either not found or already closed — load to distinguish
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM work_order_downtime_segments WHERE id = ?",
                [segment_id.into()],
            ))
            .await?;
        if exists.is_none() {
            return Err(AppError::NotFound {
                entity: "WoDowntimeSegment".into(),
                id: segment_id.to_string(),
            });
        }
        return Err(AppError::ValidationFailed(vec![
            "Le segment de temps d'arrêt est déjà fermé.".to_string(),
        ]));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {DOWNTIME_COLS} FROM work_order_downtime_segments WHERE id = ?"),
            [segment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoDowntimeSegment".into(),
            id: segment_id.to_string(),
        })?;
    map_downtime_segment(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_delay_segments
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_delay_segments(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoDelaySegment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {DELAY_COLS} FROM work_order_delay_segments \
                 WHERE work_order_id = ? ORDER BY started_at ASC"
            ),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_delay_segment).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) list_downtime_segments
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_downtime_segments(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoDowntimeSegment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {DOWNTIME_COLS} FROM work_order_downtime_segments \
                 WHERE work_order_id = ? ORDER BY started_at ASC"
            ),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_downtime_segment).collect()
}
