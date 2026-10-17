//! Background poller for DI SLA breach notifications.
//!
//! Selects only SQL-filtered breach candidates (past frozen deadline, not yet
//! notified), emits `di_sla_breach`, then sets persistent notify timestamps.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use tokio::time::{interval, Duration};

use crate::notifications::emitter::{emit_device_event_with, NotificationEventInput, NotificationMessage};

use super::domain::{map_intervention_request, IR_COLS, IR_JOINS, IR_JOIN_COLS};
use super::sla::{compute_sla_status, mark_resolution_breach_notified, mark_response_breach_notified};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 300;

/// Open requests: the DI module's one rule (`di::stats::OPEN_SQL`).
const OPEN_SQL: &str = super::stats::OPEN_SQL;

pub async fn start_di_sla_poller(db: DatabaseConnection) {
    let mut secs = read_poll_interval_secs(&db).await;
    let mut ticker = interval(Duration::from_secs(secs.max(30)));

    loop {
        ticker.tick().await;

        let next = read_poll_interval_secs(&db).await;
        if next != secs && next >= 30 {
            secs = next;
            ticker = interval(Duration::from_secs(secs));
            ticker.tick().await;
        }

        if let Err(err) = run_sla_poll_tick(&db).await {
            tracing::error!(error = %err, "di::sla_poller tick failed");
        }
    }
}

async fn read_poll_interval_secs(db: &DatabaseConnection) -> u64 {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings \
             WHERE setting_key = 'di.sla_poll_interval_seconds' \
             LIMIT 1"
                .to_string(),
        ))
        .await
        .ok()
        .flatten();

    let Some(row) = row else {
        return DEFAULT_POLL_INTERVAL_SECS;
    };

    let Ok(raw) = row.try_get::<String>("", "setting_value_json") else {
        return DEFAULT_POLL_INTERVAL_SECS;
    };

    let trimmed = raw.trim().trim_matches('"');
    trimmed.parse::<u64>().unwrap_or(DEFAULT_POLL_INTERVAL_SECS).max(30)
}

/// One poll tick — public for tests.
pub async fn run_sla_poll_tick(db: &DatabaseConnection) -> Result<(), String> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE {OPEN_SQL} \
                   AND ( \
                     (ir.screened_at IS NULL \
                      AND ir.sla_response_deadline IS NOT NULL \
                      AND ir.sla_response_breach_notified_at IS NULL \
                      AND julianday('now') > julianday(ir.sla_response_deadline)) \
                     OR \
                     (ir.converted_at IS NULL \
                      AND ir.sla_resolution_deadline IS NOT NULL \
                      AND ir.sla_resolution_breach_notified_at IS NULL \
                      AND julianday('now') > julianday(ir.sla_resolution_deadline)) \
                   )"
            ),
        ))
        .await
        .map_err(|e| e.to_string())?;

    for row in rows {
        let di = map_intervention_request(&row).map_err(|e| e.to_string())?;
        let status = compute_sla_status(db, &di).await.map_err(|e| e.to_string())?;

        if status.is_response_breached && di.sla_response_breach_notified_at.is_none() {
            emit_breach(db, &di, "response", status.sla_deadline.as_deref()).await;
            mark_response_breach_notified(db, di.id)
                .await
                .map_err(|e| e.to_string())?;
        }
        if status.is_resolution_breached && di.sla_resolution_breach_notified_at.is_none() {
            emit_breach(db, &di, "resolution", status.resolution_deadline.as_deref()).await;
            mark_resolution_breach_notified(db, di.id)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

async fn emit_breach(
    db: &DatabaseConnection,
    di: &super::domain::InterventionRequest,
    kind: &str,
    deadline: Option<&str>,
) {
    let dedupe_key = format!("di-sla-{kind}-breach-{}", di.id);
    let title = match kind {
        "response" => format!("SLA response breach — {}", di.code),
        _ => format!("SLA resolution breach — {}", di.code),
    };
    let body = match deadline {
        Some(d) => format!(
            "DI {} ({}) exceeded the {} SLA. Deadline was {}.",
            di.code, di.title, kind, d
        ),
        None => format!("DI {} ({}) exceeded the {} SLA.", di.code, di.title, kind),
    };

    let payload = serde_json::json!({
        "org_node_id": di.org_node_id,
        "di_id": di.id,
        "di_code": di.code,
        "breach_kind": kind,
    });

    // Every device's poller detects the same breach: the event is kept on each device, with the
    // same identity everywhere (built from the request's sync id).
    let result: crate::errors::AppResult<()> = async {
        let txn = db.begin().await?;
        crate::sync::capture::begin_suppression(&txn).await?;
        let identity = crate::sync::identity::optional_entity_sync_id(&txn, "intervention_requests", di.id)
            .await?
            .map(|sync_id| format!("di_sla_breach|{kind}|{sync_id}"));
        let message = NotificationMessage::new(
            if kind == "response" {
                "di_sla_breach_response"
            } else {
                "di_sla_breach_resolution"
            },
            serde_json::json!({ "code": di.code, "title": di.title, "deadline": deadline }),
        )
        .with_urgency(super::notifications::di_urgency(di));
        emit_device_event_with(
            &txn,
            NotificationEventInput {
                source_module: "intervention_requests".into(),
                source_record_id: Some(di.id.to_string()),
                event_code: "di_sla_breach".into(),
                category_code: "di_sla_breach".into(),
                severity: "warning".into(),
                dedupe_key: Some(dedupe_key),
                payload_json: Some(payload.to_string()),
                title,
                body: Some(body),
                action_url: Some(format!("/requests?openDi={}", di.id)),
            },
            identity.as_deref(),
            Some(&message),
        )
        .await?;
        crate::sync::capture::end_suppression(&txn).await?;
        txn.commit().await?;
        Ok(())
    }
    .await;
    if let Err(err) = result {
        tracing::error!(di_id = di.id, error = %err, "di::sla_poller breach notification failed");
    }
}
