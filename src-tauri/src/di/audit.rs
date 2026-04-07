//! DI audit event writer and reader.
//!
//! Phase 2 - Sub-phase 04 - File 04 - Sprint S1.
//!
//! `di_change_events` is append-only. No update or delete functions exist.
//! `record_di_change_event` is fire-and-log: it never propagates insert errors
//! to the caller, ensuring the primary workflow is never blocked by audit failures.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single row in `di_change_events`, returned by list queries.
#[derive(Debug, Clone, Serialize, Deserialize, FromQueryResult)]
pub struct DiChangeEvent {
    pub id: i64,
    pub di_id: Option<i64>,
    pub action: String,
    pub actor_id: Option<i64>,
    pub acted_at: String,
    pub summary: Option<String>,
    pub details_json: Option<String>,
    pub requires_step_up: i32,
    pub apply_result: String,
}

/// Input for recording a change event. Callers build this and pass to
/// `record_di_change_event`.
#[derive(Debug, Clone)]
pub struct DiAuditInput {
    pub di_id: Option<i64>,
    pub action: String,
    pub actor_id: Option<i64>,
    pub summary: Option<String>,
    pub details_json: Option<String>,
    pub requires_step_up: bool,
    /// "applied" | "blocked" | "partial"
    pub apply_result: String,
}

/// Filter for the admin-facing full audit log query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiAuditFilter {
    pub action: Option<String>,
    pub actor_id: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) record_di_change_event — fire-and-log writer
// ═══════════════════════════════════════════════════════════════════════════════

/// Insert a single audit event into `di_change_events`.
///
/// **Fire-and-log semantics**: if the INSERT fails, the error is logged at `warn`
/// level but is NOT returned. This ensures that a transient audit-write failure
/// never blocks the primary DI workflow.
pub async fn record_di_change_event(db: &DatabaseConnection, input: DiAuditInput) {
    let now = chrono::Utc::now().to_rfc3339();

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO di_change_events
                   (di_id, action, actor_id, acted_at, summary, details_json, requires_step_up, apply_result)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                input.di_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::Int(None)),
                input.action.clone().into(),
                input.actor_id.map(sea_orm::Value::from).unwrap_or(sea_orm::Value::Int(None)),
                now.into(),
                input
                    .summary
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::String(None)),
                input
                    .details_json
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::String(None)),
                i32::from(input.requires_step_up).into(),
                input.apply_result.clone().into(),
            ],
        ))
        .await;

    if let Err(e) = result {
        tracing::warn!(
            di_id = ?input.di_id,
            action = %input.action,
            apply_result = %input.apply_result,
            error = %e,
            "audit::record_di_change_event failed (non-fatal)"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) list_di_change_events — per-DI timeline query
// ═══════════════════════════════════════════════════════════════════════════════

/// Return audit events for a single DI, ordered chronologically (oldest first).
pub async fn list_di_change_events(
    db: &DatabaseConnection,
    di_id: i64,
    limit: i64,
) -> crate::errors::AppResult<Vec<DiChangeEvent>> {
    let rows = DiChangeEvent::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"SELECT id, di_id, action, actor_id, acted_at, summary, details_json,
                 requires_step_up, apply_result
            FROM di_change_events
           WHERE di_id = ?
           ORDER BY acted_at ASC, id ASC
           LIMIT ?",
        [di_id.into(), limit.into()],
    ))
    .all(db)
    .await?;

    Ok(rows)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_all_change_events — admin-facing global audit log
// ═══════════════════════════════════════════════════════════════════════════════

/// Return audit events across all DIs with optional filters.
/// Used by di.admin users in the global audit log view.
pub async fn list_all_change_events(
    db: &DatabaseConnection,
    filter: DiAuditFilter,
) -> crate::errors::AppResult<Vec<DiChangeEvent>> {
    let limit = filter.limit.unwrap_or(50).min(500);
    let offset = filter.offset.unwrap_or(0);

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref action) = filter.action {
        conditions.push("action = ?".to_string());
        values.push(action.clone().into());
    }
    if let Some(actor_id) = filter.actor_id {
        conditions.push("actor_id = ?".to_string());
        values.push(actor_id.into());
    }
    if let Some(ref date_from) = filter.date_from {
        conditions.push("acted_at >= ?".to_string());
        values.push(date_from.clone().into());
    }
    if let Some(ref date_to) = filter.date_to {
        conditions.push("acted_at <= ?".to_string());
        values.push(date_to.clone().into());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    values.push(limit.into());
    values.push(offset.into());

    let sql = format!(
        r"SELECT id, di_id, action, actor_id, acted_at, summary, details_json,
                 requires_step_up, apply_result
            FROM di_change_events
           {where_clause}
           ORDER BY acted_at DESC, id DESC
           LIMIT ? OFFSET ?"
    );

    let rows = DiChangeEvent::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &sql,
        values,
    ))
    .all(db)
    .await?;

    Ok(rows)
}
