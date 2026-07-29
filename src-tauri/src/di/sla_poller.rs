//! Background poller for DI SLA breach notifications.
//!
//! Selects only SQL-filtered breach candidates (past frozen deadline, not yet
//! notified), emits `di_sla_breach`, then sets persistent notify timestamps.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::time::{interval, Duration};

use crate::notifications::emitter::{emit_event, NotificationEventInput};

use super::domain::map_intervention_request;
use super::sla::{
    compute_sla_status, mark_resolution_breach_notified, mark_response_breach_notified,
};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 300;

const OPEN_STATUSES: &str = "(\
    'submitted', \
    'pending_review', \
    'screened', \
    'awaiting_approval', \
    'approved_for_planning'\
)";

const IR_COLS: &str = "\
    ir.id, ir.code, ir.asset_id, ir.sub_asset_ref, ir.org_node_id, ir.status, \
    ir.title, ir.description, ir.origin_type, ir.request_type, ir.symptom_code_id, \
    ir.impact_level, ir.production_impact, ir.safety_flag, ir.environmental_flag, \
    ir.quality_flag, ir.reported_urgency, ir.validated_urgency, \
    ir.observed_at, ir.submitted_at, \
    ir.review_team_id, ir.reviewer_id, ir.screened_at, ir.approved_at, \
    ir.deferred_until, ir.declined_at, ir.closed_at, ir.archived_at, \
    ir.converted_to_wo_id, ir.converted_at, \
    ir.sla_rule_id, ir.sla_target_response_hours, ir.sla_target_resolution_hours, \
    ir.sla_escalation_threshold_hours, ir.sla_response_deadline, ir.sla_resolution_deadline, \
    ir.sla_response_breach_notified_at, ir.sla_resolution_breach_notified_at, \
    ir.reviewer_note, ir.classification_code_id, \
    ir.is_recurrence_flag, ir.recurrence_di_id, ir.source_inspection_anomaly_id, \
    ir.row_version, ir.submitter_id, ir.created_at, ir.updated_at";

const IR_JOIN_COLS: &str = "\
    eq.asset_id_code AS asset_code, eq.name AS asset_label, \
    org.code AS org_node_code, org.name AS org_node_label, \
    COALESCE(us.display_name, us.username) AS submitter_display_name, \
    COALESCE(ur.display_name, ur.username) AS reviewer_display_name, \
    wo.code AS converted_to_wo_code, wo.title AS converted_to_wo_title";

const IR_JOINS: &str = "\
    LEFT JOIN equipment eq ON eq.id = ir.asset_id \
    LEFT JOIN org_nodes org ON org.id = ir.org_node_id \
    LEFT JOIN user_accounts us ON us.id = ir.submitter_id \
    LEFT JOIN user_accounts ur ON ur.id = ir.reviewer_id \
    LEFT JOIN work_orders wo ON wo.id = ir.converted_to_wo_id";

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
    trimmed
        .parse::<u64>()
        .unwrap_or(DEFAULT_POLL_INTERVAL_SECS)
        .max(30)
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
                 WHERE ir.status IN {OPEN_STATUSES} \
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
        let status = compute_sla_status(db, &di)
            .await
            .map_err(|e| e.to_string())?;

        if status.is_response_breached && di.sla_response_breach_notified_at.is_none() {
            emit_breach(db, &di, "response", status.sla_deadline.as_deref()).await;
            mark_response_breach_notified(db, di.id)
                .await
                .map_err(|e| e.to_string())?;
        }
        if status.is_resolution_breached && di.sla_resolution_breach_notified_at.is_none() {
            emit_breach(
                db,
                &di,
                "resolution",
                status.resolution_deadline.as_deref(),
            )
            .await;
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
        None => format!(
            "DI {} ({}) exceeded the {} SLA.",
            di.code, di.title, kind
        ),
    };

    let payload = serde_json::json!({
        "org_node_id": di.org_node_id,
        "di_id": di.id,
        "di_code": di.code,
        "breach_kind": kind,
    });

    let _ = emit_event(
        db,
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
            action_url: Some(format!("/requests?di={}", di.id)),
        },
    )
    .await;
}
