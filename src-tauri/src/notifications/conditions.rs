//! Notifications that stop when their condition clears.
//!
//! A notification about a state ("request waiting for your review", "SLA breached", "PM missed")
//! is resolved once the record has left that state: it stays in the inbox, marked resolved, is no
//! longer counted as unread or "to acknowledge", and no longer escalates. The conditions of the
//! categories raised by people's actions and by the module pollers are listed here; the
//! categories of the condition watchers resolve their own events (`notifications::watchers`).
//! Informational categories (request closed or converted, PM follow-up created, planning
//! messages, budget alerts, the daily digest) describe a fact and are never resolved.
//!
//! Each device resolves its own copy from synchronised data (`resolved_at` is device-local).

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::notifications::Result;

/// `(category, source modules, table, condition still true on row t)`. The row is the event's
/// source record (`t.id = source_record_id`); `e` is the event.
pub const CONDITIONS: &[(&str, &[&str], &str, &str)] = &[
    (
        "di_pending_review",
        &["di", "intervention_requests"],
        "intervention_requests",
        "t.status = 'in_review'",
    ),
    (
        "di_returned",
        &["di", "intervention_requests"],
        "intervention_requests",
        "t.status = 'returned_for_clarification'",
    ),
    (
        "di_approved",
        &["di", "intervention_requests"],
        "intervention_requests",
        "t.status = 'approved' AND t.converted_at IS NULL",
    ),
    (
        "di_deferred",
        &["di", "intervention_requests"],
        "intervention_requests",
        "t.status = 'deferred'",
    ),
    (
        "di_sla_breach",
        &["di", "intervention_requests"],
        "intervention_requests",
        // The open statuses of `di::sla_poller`.
        "t.converted_at IS NULL AND t.status IN ('submitted', 'in_review', 'returned_for_clarification',            'awaiting_approval', 'approved', 'deferred')",
    ),
    (
        "pm_due",
        &["pm"],
        "pm_occurrences",
        "t.status IN ('forecasted', 'generated', 'ready_for_scheduling', 'scheduled', 'in_progress')",
    ),
    ("pm_missed", &["pm"], "pm_occurrences", "t.status = 'missed'"),
    ("pm_deferred", &["pm"], "pm_occurrences", "t.status = 'deferred'"),
    (
        "personnel_critical_block",
        &["personnel"],
        "personnel",
        "EXISTS (SELECT 1 FROM personnel_availability_blocks b WHERE b.personnel_id = t.id \
           AND b.is_critical = 1 AND b.end_at > strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
    ),
    (
        "cert_expiry",
        &["personnel"],
        "personnel",
        "EXISTS (SELECT 1 FROM personnel_certifications pc WHERE pc.personnel_id = t.id \
           AND pc.id = json_extract(e.payload_json, '$.certification_id') \
           AND COALESCE(substr(pc.expires_at, 1, 10), '') = COALESCE(json_extract(e.message_params_json, '$.expires_on'), substr(pc.expires_at, 1, 10)))",
    ),
];

/// Marks resolved the open events (and their notifications) whose condition no longer holds.
/// Returns the number of events resolved.
pub async fn resolve_cleared(conn: &impl ConnectionTrait) -> Result<u64> {
    let mut resolved = 0;
    for (category, modules, table, cond) in CONDITIONS {
        let marks = vec!["?"; modules.len()].join(",");
        let mut values: Vec<sea_orm::Value> = vec![(*category).into()];
        values.extend(modules.iter().map(|m| (*m).into()));
        let res = conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "UPDATE notification_events AS e SET resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                     WHERE e.resolved_at IS NULL AND e.category_code = ? AND e.source_module IN ({marks}) \
                       AND NOT EXISTS (SELECT 1 FROM {table} t \
                                       WHERE t.id = CAST(e.source_record_id AS INTEGER) AND {cond})"
                ),
                values,
            ))
            .await?;
        resolved += res.rows_affected();
    }
    resolve_notifications_of_resolved_events(conn).await?;
    Ok(resolved)
}

/// Copies the resolution of events to their notifications.
pub async fn resolve_notifications_of_resolved_events(conn: &impl ConnectionTrait) -> Result<()> {
    conn.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE notifications SET resolved_at = (SELECT e.resolved_at FROM notification_events e \
                                                  WHERE e.id = notifications.notification_event_id) \
         WHERE resolved_at IS NULL AND notification_event_id IN \
               (SELECT id FROM notification_events WHERE resolved_at IS NOT NULL)"
            .to_string(),
    ))
    .await?;
    Ok(())
}
