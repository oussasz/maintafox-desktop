//! WO audit event writer and reader.
//!
//! Phase 2 - Sub-phase 05 - File 04 - Sprint S1.
//!
//! `wo_change_events` is append-only. No update or delete functions exist.
//! `record_wo_change_event` is fire-and-log: it never propagates insert errors
//! to the caller, ensuring the primary workflow is never blocked by audit failures.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::wo::domain::WorkOrder;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single row in `wo_change_events`, returned by list queries.
#[derive(Debug, Clone, Serialize, Deserialize, FromQueryResult)]
pub struct WoChangeEvent {
    pub id: i64,
    pub wo_id: Option<i64>,
    pub action: String,
    pub actor_id: Option<i64>,
    pub actor_display_name: Option<String>,
    pub acted_at: String,
    pub summary: Option<String>,
    pub details_json: Option<String>,
    pub requires_step_up: i32,
    pub apply_result: String,
    /// Where the row comes from: `wo` (this table) or `audit` (an `audit_events` row whose object
    /// is this work order, merged into the per-WO history by [`list_wo_history`]).
    pub source: String,
}

/// Input for recording a change event. Callers build this and pass to
/// `record_wo_change_event`.
#[derive(Debug, Clone)]
pub struct WoAuditInput {
    pub wo_id: Option<i64>,
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
pub struct WoAuditFilter {
    pub action: Option<String>,
    pub actor_id: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub wo_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) record_wo_change_event — fire-and-log writer
// ═══════════════════════════════════════════════════════════════════════════════

/// Insert a single audit event into `wo_change_events`.
///
/// **Fire-and-log semantics**: if the INSERT fails, the error is logged at `warn`
/// level but is NOT returned. This ensures that a transient audit-write failure
/// never blocks the primary WO workflow.
pub async fn record_wo_change_event(db: &DatabaseConnection, input: WoAuditInput) {
    if let Err(e) = insert_wo_change_event(db, &input).await {
        tracing::warn!(
            wo_id = ?input.wo_id,
            action = %input.action,
            apply_result = %input.apply_result,
            error = %e,
            "audit::record_wo_change_event failed (non-fatal)"
        );
    }
}

/// Insert one audit event on `conn` (a transaction included) and return the failure: for a change
/// whose audit entry must be written with it or not at all (the caller rolls back on error).
pub async fn insert_wo_change_event<C: ConnectionTrait>(conn: &C, input: &WoAuditInput) -> Result<(), sea_orm::DbErr> {
    let now = crate::clock::now().to_rfc3339();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO wo_change_events
               (wo_id, action, actor_id, acted_at, summary, details_json, requires_step_up, apply_result)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input
                .wo_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::BigInt(None)),
            input.action.clone().into(),
            input
                .actor_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::BigInt(None)),
            now.into(),
            input
                .summary
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::String(None)),
            input
                .details_json
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::String(None)),
            i32::from(input.requires_step_up).into(),
            input.apply_result.clone().into(),
        ],
    ))
    .await
    .map(|_| ())
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) list_wo_change_events — per-WO timeline query
// ═══════════════════════════════════════════════════════════════════════════════

/// Largest page of the per-WO history.
pub const WO_CHANGE_EVENTS_MAX_LIMIT: i64 = 500;

/// Return the `limit` MOST RECENT audit events of a single WO, ordered chronologically
/// (oldest first). Capping keeps the latest events: the screen offers "show older" when the
/// page is full, instead of silently hiding what happened last.
pub async fn list_wo_change_events(
    db: &DatabaseConnection,
    wo_id: i64,
    limit: i64,
) -> crate::errors::AppResult<Vec<WoChangeEvent>> {
    let limit = limit.clamp(1, WO_CHANGE_EVENTS_MAX_LIMIT);
    let rows = WoChangeEvent::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"SELECT * FROM (
            SELECT e.id, e.wo_id, e.action, e.actor_id,
                   COALESCE(ua.display_name, ua.username) AS actor_display_name,
                   e.acted_at, e.summary, e.details_json,
                   e.requires_step_up, e.apply_result, 'wo' AS source
              FROM wo_change_events e
              LEFT JOIN user_accounts ua ON ua.id = e.actor_id
             WHERE e.wo_id = ?
             ORDER BY e.acted_at DESC, e.id DESC
             LIMIT ?
          ) latest
          ORDER BY latest.acted_at ASC, latest.id ASC",
        [wo_id.into(), limit.into()],
    ))
    .all(db)
    .await?;

    Ok(rows)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_all_wo_change_events — admin-facing global audit log
// ═══════════════════════════════════════════════════════════════════════════════

/// Return audit events across all WOs with optional filters.
/// Used by ot.admin users in the global audit log view.
pub async fn list_all_wo_change_events(
    db: &DatabaseConnection,
    filter: WoAuditFilter,
) -> crate::errors::AppResult<Vec<WoChangeEvent>> {
    list_all_wo_change_events_in_scope(db, filter, &super::access::ScopeSql::everywhere()).await
}

/// [`list_all_wo_change_events`] limited to the work orders of the reader's scope
/// (`wo::access::ScopeSql` on the event's work order; an event without a work order, or whose
/// work order is gone, has no site: plant-wide readers only).
pub async fn list_all_wo_change_events_in_scope(
    db: &DatabaseConnection,
    filter: WoAuditFilter,
    scope: &super::access::ScopeSql,
) -> crate::errors::AppResult<Vec<WoChangeEvent>> {
    let limit = filter.limit.unwrap_or(50).min(500);
    let offset = filter.offset.unwrap_or(0);

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref action) = filter.action {
        conditions.push("e.action = ?".to_string());
        values.push(action.clone().into());
    }
    if let Some(actor_id) = filter.actor_id {
        conditions.push("e.actor_id = ?".to_string());
        values.push(actor_id.into());
    }
    if let Some(ref date_from) = filter.date_from {
        conditions.push("e.acted_at >= ?".to_string());
        values.push(date_from.clone().into());
    }
    if let Some(ref date_to) = filter.date_to {
        conditions.push("e.acted_at <= ?".to_string());
        values.push(date_to.clone().into());
    }
    if let Some(wo_id) = filter.wo_id {
        conditions.push("e.wo_id = ?".to_string());
        values.push(wo_id.into());
    }

    // Last condition: its binds follow the filters' and precede LIMIT / OFFSET.
    conditions.push(format!("({})", scope.predicate));
    values.extend(scope.binds.iter().cloned());

    let where_clause = format!("WHERE {}", conditions.join(" AND "));

    values.push(limit.into());
    values.push(offset.into());

    let sql = format!(
        r"SELECT e.id, e.wo_id, e.action, e.actor_id,
                 COALESCE(ua.display_name, ua.username) AS actor_display_name,
                 e.acted_at, e.summary, e.details_json,
                 e.requires_step_up, e.apply_result, 'wo' AS source
            FROM wo_change_events e
            LEFT JOIN user_accounts ua ON ua.id = e.actor_id
            LEFT JOIN work_orders wo ON wo.id = e.wo_id
           {where_clause}
           ORDER BY e.acted_at DESC, e.id DESC
           LIMIT ? OFFSET ?"
    );

    let rows = WoChangeEvent::find_by_statement(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .all(db)
        .await?;

    Ok(rows)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) Field-level changes (old → new) recorded with each change event
// ═══════════════════════════════════════════════════════════════════════════════

/// `details_json.source` of an `audit_events` copy of a change event ([`record_wo_change_with_diff`]):
/// the per-WO history already shows the change event, so it skips the copy.
pub const AUDIT_COPY_SOURCE: &str = "wo_change_events";

fn opt_text(v: Option<&str>) -> serde_json::Value {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => serde_json::Value::String(s.to_string()),
        None => serde_json::Value::Null,
    }
}

fn opt_f64(v: Option<f64>) -> serde_json::Value {
    v.map_or(serde_json::Value::Null, serde_json::Value::from)
}

fn equipment_text(wo: &WorkOrder) -> serde_json::Value {
    match (wo.asset_code.as_deref(), wo.asset_label.as_deref()) {
        (Some(code), Some(name)) if !name.trim().is_empty() => serde_json::Value::String(format!("{code} — {name}")),
        (Some(code), _) => serde_json::Value::String(code.to_string()),
        (None, Some(name)) => serde_json::Value::String(name.to_string()),
        (None, None) => serde_json::Value::Null,
    }
}

/// The values of a work order an auditor asks about, as they read on screen (labels, names and
/// codes, not ids), keyed by a stable field code the screen translates (`labels.field.<code>`).
fn audited_values(wo: &WorkOrder) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "status",
            opt_text(wo.status_label.as_deref().or(wo.status_code.as_deref())),
        ),
        ("title", opt_text(Some(wo.title.as_str()))),
        ("description", opt_text(wo.description.as_deref())),
        (
            "urgency",
            opt_text(wo.urgency_label.as_deref().or(wo.urgency_code.as_deref())),
        ),
        ("equipment", equipment_text(wo)),
        (
            "primary_responsible",
            opt_text(
                wo.responsible_display_name
                    .as_deref()
                    .or(wo.responsible_username.as_deref()),
            ),
        ),
        (
            "assigned_team",
            opt_text(
                wo.assigned_team_label
                    .as_deref()
                    .or(wo.assigned_team_name.as_deref())
                    .or(wo.assigned_team_code.as_deref()),
            ),
        ),
        ("planned_start", opt_text(wo.planned_start.as_deref())),
        ("planned_end", opt_text(wo.planned_end.as_deref())),
        ("scheduled_at", opt_text(wo.scheduled_at.as_deref())),
        ("actual_start", opt_text(wo.actual_start.as_deref())),
        ("actual_end", opt_text(wo.actual_end.as_deref())),
        (
            "mechanically_completed_at",
            opt_text(wo.mechanically_completed_at.as_deref()),
        ),
        (
            "technically_verified_at",
            opt_text(wo.technically_verified_at.as_deref()),
        ),
        ("closed_at", opt_text(wo.closed_at.as_deref())),
        ("cancelled_at", opt_text(wo.cancelled_at.as_deref())),
        ("expected_duration_hours", opt_f64(wo.expected_duration_hours)),
        (
            "planned_crew_size",
            wo.planned_crew_size
                .map_or(serde_json::Value::Null, serde_json::Value::from),
        ),
        ("requires_shutdown", serde_json::Value::Bool(wo.requires_shutdown)),
        ("requires_permit", serde_json::Value::Bool(wo.requires_permit)),
        ("service_cost", opt_f64(wo.service_cost_input)),
        ("cancel_reason", opt_text(wo.cancel_reason.as_deref())),
    ]
}

/// The audited fields whose value differs between `before` and `after`, in a fixed order:
/// `[{"field": "closed_at", "old": …, "new": …}, …]`.
pub fn wo_field_changes(before: &WorkOrder, after: &WorkOrder) -> Vec<serde_json::Value> {
    audited_values(before)
        .into_iter()
        .zip(audited_values(after))
        .filter(|((_, old), (_, new))| old != new)
        .map(|((field, old), (_, new))| serde_json::json!({ "field": field, "old": old, "new": new }))
        .collect()
}

/// `details` with the field changes added under `changes` (a details value that is not an object
/// is kept under `detail`). Unchanged when there is no change to record.
pub fn details_with_changes(
    details: Option<serde_json::Value>,
    changes: Vec<serde_json::Value>,
) -> Option<serde_json::Value> {
    if changes.is_empty() {
        return details;
    }
    let mut obj = match details {
        Some(serde_json::Value::Object(map)) => map,
        Some(other) => {
            let mut map = serde_json::Map::new();
            map.insert("detail".into(), other);
            map
        }
        None => serde_json::Map::new(),
    };
    obj.insert("changes".into(), serde_json::Value::Array(changes));
    Some(serde_json::Value::Object(obj))
}

/// Records a change event with the fields it changed (`before` read before the change; the work
/// order is read again after it, with the same query) and, when fields changed, the same event in the audit
/// trail (`audit_events`, action `wo.<action>`, object the work order), so the audit log answers
/// "who changed field X of OT Y, from what to what". Fire-and-log like [`record_wo_change_event`].
/// Events stored before this existed are not touched (audit chain): they simply show no change.
pub async fn record_wo_change_with_diff(db: &DatabaseConnection, mut input: WoAuditInput, before: Option<&WorkOrder>) {
    let after = match (before, input.wo_id) {
        (Some(_), Some(id)) => super::queries::get_work_order(db, id).await.ok().flatten(),
        _ => None,
    };
    let changes = match (before, after.as_ref()) {
        (Some(b), Some(a)) => wo_field_changes(b, a),
        _ => Vec::new(),
    };
    let has_changes = !changes.is_empty();
    let base = input
        .details_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    let details = details_with_changes(base, changes);
    if has_changes {
        input.details_json = details.as_ref().map(serde_json::Value::to_string);
    }
    let action = input.action.clone();
    let actor_id = input.actor_id;
    let step_up = input.requires_step_up;
    record_wo_change_event(db, input).await;

    // The close-out writes its own audit event (`wo.closed`, with the fields it changed).
    let Some(after) = after.filter(|_| has_changes && action != "closed") else {
        return;
    };
    let mut copy = details.unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = copy.as_object_mut() {
        obj.insert("source".into(), AUDIT_COPY_SOURCE.into());
        obj.insert("wo_code".into(), after.code.clone().into());
    }
    let written = crate::audit::writer::write_audit_event(
        db,
        crate::audit::writer::AuditEventInput {
            action_code: format!("wo.{action}"),
            target_type: Some("work_order".to_string()),
            target_id: Some(after.id.to_string()),
            actor_id,
            auth_context: if step_up { "step_up" } else { "session" }.to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(copy),
        },
    )
    .await;
    if let Err(e) = written {
        tracing::warn!(wo_id = after.id, action = %action, error = %e, "WO change audit copy failed (non-fatal)");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) list_wo_history — the WO's change events and its audit-trail events, merged
// ═══════════════════════════════════════════════════════════════════════════════

/// The `limit` most recent events of one work order, oldest first: its change events
/// (`wo_change_events`) and the audit-trail events whose object is this work order
/// (`audit_events`: a close-out recorded without a change event, an imported history…). An audit
/// event that only repeats a change event is left out: the copies written by
/// [`record_wo_change_with_diff`], and the close-out's `wo.closed` when the change event "closed"
/// was recorded within the same minute. Audit events received from another device before ids
/// travelled as sync ids name that device's work order: left out.
pub async fn list_wo_history(
    db: &DatabaseConnection,
    wo_id: i64,
    limit: i64,
) -> crate::errors::AppResult<Vec<WoChangeEvent>> {
    let limit = limit.clamp(1, WO_CHANGE_EVENTS_MAX_LIMIT);
    let mut rows = WoChangeEvent::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"SELECT * FROM (
            SELECT e.id, e.wo_id, e.action, e.actor_id,
                   COALESCE(ua.display_name, ua.username) AS actor_display_name,
                   e.acted_at, e.summary, e.details_json,
                   e.requires_step_up, e.apply_result, 'wo' AS source
              FROM wo_change_events e
              LEFT JOIN user_accounts ua ON ua.id = e.actor_id
             WHERE e.wo_id = ?
            UNION ALL
            SELECT ae.id, ? AS wo_id, ae.action_code AS action, ae.actor_id,
                   COALESCE(ua.display_name, ua.username) AS actor_display_name,
                   ae.happened_at AS acted_at,
                   CASE WHEN json_valid(ae.details_json)
                        THEN json_extract(ae.details_json, '$.summary') END AS summary,
                   ae.details_json,
                   CASE WHEN ae.auth_context = 'step_up' THEN 1 ELSE 0 END AS requires_step_up,
                   ae.result AS apply_result, 'audit' AS source
              FROM audit_events ae
              LEFT JOIN user_accounts ua ON ua.id = ae.actor_id
             WHERE ae.target_type = 'work_order'
               AND ae.target_id = ?
               AND NOT EXISTS (SELECT 1 FROM sync_history_foreign_rows f
                                WHERE f.entity_type = 'audit_events' AND f.entity_sync_id = ae.entity_sync_id)
               AND COALESCE(CASE WHEN json_valid(ae.details_json)
                                 THEN json_extract(ae.details_json, '$.source') END, '') <> ?
               AND NOT (ae.action_code = 'wo.closed' AND EXISTS (
                        SELECT 1 FROM wo_change_events c
                         WHERE c.wo_id = ? AND c.action = 'closed' AND c.apply_result = 'applied'
                           AND abs(julianday(c.acted_at) - julianday(ae.happened_at)) < (1.0 / 1440)))
          ) merged
          ORDER BY julianday(merged.acted_at) DESC, merged.source DESC, merged.id DESC
          LIMIT ?",
        [
            wo_id.into(),
            wo_id.into(),
            wo_id.to_string().into(),
            AUDIT_COPY_SOURCE.into(),
            wo_id.into(),
            limit.into(),
        ],
    ))
    .all(db)
    .await?;
    rows.reverse();
    Ok(rows)
}
