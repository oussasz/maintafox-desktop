//! Org-specific append-only audit trail.
//!
//! Every structural org change (publish, move, deactivate, responsibility,
//! entity binding) writes an immutable row to `org_change_events`.
//! Blocked validation attempts also write a row with `apply_result = 'blocked'`.
//!
//! Rows are never updated or deleted. This is a governance requirement.
//!
//! Sub-phase 01 — File 04 — Sprint S2.

use crate::errors::AppResult;
use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ─── Public types ─────────────────────────────────────────────────────────────

/// Input for recording an org structural change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgAuditEventInput {
    pub entity_kind: String,
    pub entity_id: Option<i64>,
    pub change_type: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub preview_summary_json: Option<String>,
    pub changed_by_id: Option<i64>,
    pub requires_step_up: bool,
    pub apply_result: String,
}

/// A persisted org change event row, returned by list queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgChangeEvent {
    pub id: i64,
    pub entity_kind: String,
    pub entity_id: Option<i64>,
    pub change_type: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub preview_summary_json: Option<String>,
    pub changed_by_id: Option<i64>,
    pub changed_at: String,
    pub requires_step_up: bool,
    pub apply_result: String,
}

// ─── Write operations ─────────────────────────────────────────────────────────

/// Record an org structural change event.
///
/// Fire-and-forget semantics: if the insert fails, a `tracing::error!` is emitted
/// but the caller's operation is NOT rolled back.
pub async fn record_org_change(
    db: &impl ConnectionTrait,
    input: OrgAuditEventInput,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let step_up_val: i32 = if input.requires_step_up { 1 } else { 0 };
    let change_type_log = input.change_type.clone();

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_change_events \
             (entity_kind, entity_id, change_type, before_json, after_json, \
              preview_summary_json, changed_by_id, changed_at, requires_step_up, apply_result) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                input.entity_kind.into(),
                input
                    .entity_id
                    .map_or(sea_orm::Value::Int(None), |v| (v as i32).into()),
                input.change_type.into(),
                input
                    .before_json
                    .map_or(sea_orm::Value::String(None), Into::into),
                input
                    .after_json
                    .map_or(sea_orm::Value::String(None), Into::into),
                input
                    .preview_summary_json
                    .map_or(sea_orm::Value::String(None), Into::into),
                input
                    .changed_by_id
                    .map_or(sea_orm::Value::Int(None), |v| (v as i32).into()),
                now.into(),
                step_up_val.into(),
                input.apply_result.into(),
            ],
        ))
        .await;

    if let Err(e) = result {
        tracing::error!(
            change_type = %change_type_log,
            error = %e,
            "org_audit::record_org_change failed"
        );
    }

    Ok(())
}

// ─── Read operations ──────────────────────────────────────────────────────────

/// List org change events with optional filters.
///
/// Results are ordered newest-first. `limit` defaults to 50 if not specified.
pub async fn list_org_change_events(
    db: &impl ConnectionTrait,
    limit: Option<i64>,
    entity_kind: Option<&str>,
    entity_id: Option<i64>,
) -> AppResult<Vec<OrgChangeEvent>> {
    let effective_limit = limit.unwrap_or(50);

    // Build dynamic WHERE clause based on optional filters
    let mut sql = String::from(
        "SELECT id, entity_kind, entity_id, change_type, before_json, after_json, \
         preview_summary_json, changed_by_id, changed_at, requires_step_up, apply_result \
         FROM org_change_events WHERE 1=1",
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(kind) = entity_kind {
        sql.push_str(" AND entity_kind = ?");
        values.push(kind.into());
    }
    if let Some(eid) = entity_id {
        sql.push_str(" AND entity_id = ?");
        values.push((eid as i32).into());
    }

    sql.push_str(" ORDER BY id DESC LIMIT ?");
    values.push((effective_limit as i32).into());

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            values,
        ))
        .await?;

    let events: Vec<OrgChangeEvent> = rows
        .into_iter()
        .map(|row| {
            OrgChangeEvent {
                id: row.try_get::<i64>("", "id").unwrap_or(0),
                entity_kind: row.try_get("", "entity_kind").unwrap_or_default(),
                entity_id: row.try_get::<Option<i64>>("", "entity_id").unwrap_or(None),
                change_type: row.try_get("", "change_type").unwrap_or_default(),
                before_json: row
                    .try_get::<Option<String>>("", "before_json")
                    .unwrap_or(None),
                after_json: row
                    .try_get::<Option<String>>("", "after_json")
                    .unwrap_or(None),
                preview_summary_json: row
                    .try_get::<Option<String>>("", "preview_summary_json")
                    .unwrap_or(None),
                changed_by_id: row
                    .try_get::<Option<i64>>("", "changed_by_id")
                    .unwrap_or(None),
                changed_at: row.try_get("", "changed_at").unwrap_or_default(),
                requires_step_up: row
                    .try_get::<i32>("", "requires_step_up")
                    .unwrap_or(0)
                    != 0,
                apply_result: row.try_get("", "apply_result").unwrap_or_default(),
            }
        })
        .collect();

    Ok(events)
}
