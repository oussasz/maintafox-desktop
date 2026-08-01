//! DI lifecycle notification emitters (fire-and-log).

use sea_orm::DatabaseConnection;

use crate::notifications::emitter::{emit_event, NotificationEventInput};

use super::domain::InterventionRequest;

fn di_action_url(di_id: i64) -> String {
    format!("/requests?openDi={di_id}")
}

async fn emit_di_lifecycle(
    db: &DatabaseConnection,
    di: &InterventionRequest,
    event_code: &str,
    category_code: &str,
    severity: &str,
    title: String,
    body: Option<String>,
) {
    let dedupe_key = format!("di-{event_code}-{}", di.id);
    let input = NotificationEventInput {
        source_module: "di".into(),
        source_record_id: Some(di.id.to_string()),
        event_code: event_code.into(),
        category_code: category_code.into(),
        severity: severity.into(),
        dedupe_key: Some(dedupe_key),
        payload_json: None,
        title,
        body,
        action_url: Some(di_action_url(di.id)),
    };
    if let Err(err) = emit_event(db, input).await {
        tracing::warn!(error = %err, di_id = di.id, event_code, "DI notification emit failed");
    }
}

pub async fn notify_entered_in_review(db: &DatabaseConnection, di: &InterventionRequest) {
    emit_di_lifecycle(
        db,
        di,
        "di_entered_in_review",
        "di_pending_review",
        "info",
        format!("DI en revue — {}", di.code),
        Some(format!("{} est entrée en file de revue.", di.title)),
    )
    .await;
}

pub async fn notify_returned(db: &DatabaseConnection, di: &InterventionRequest) {
    emit_di_lifecycle(
        db,
        di,
        "di_returned",
        "di_returned",
        "warning",
        format!("DI renvoyée — {}", di.code),
        Some(format!(
            "{} nécessite des précisions du demandeur.",
            di.title
        )),
    )
    .await;
}

pub async fn notify_approved(db: &DatabaseConnection, di: &InterventionRequest) {
    emit_di_lifecycle(
        db,
        di,
        "di_approved",
        "di_approved",
        "info",
        format!("DI approuvée — {}", di.code),
        Some(format!(
            "{} est autorisée ; conversion en OT possible.",
            di.title
        )),
    )
    .await;
}

pub async fn notify_closed(db: &DatabaseConnection, di: &InterventionRequest) {
    let disposition = di
        .disposition_code
        .as_deref()
        .unwrap_or("closed");
    emit_di_lifecycle(
        db,
        di,
        "di_closed",
        "di_closed",
        "info",
        format!("DI clôturée — {}", di.code),
        Some(format!(
            "{} clôturée (disposition: {disposition}).",
            di.title
        )),
    )
    .await;
}

pub async fn notify_converted(db: &DatabaseConnection, di: &InterventionRequest) {
    let wo = di
        .converted_to_wo_code
        .as_deref()
        .unwrap_or("OT");
    emit_di_lifecycle(
        db,
        di,
        "di_converted",
        "di_converted",
        "info",
        format!("DI convertie — {} → {wo}", di.code),
        Some(format!("{} a été convertie en ordre de travail.", di.title)),
    )
    .await;
}

pub async fn notify_deferred(db: &DatabaseConnection, di: &InterventionRequest) {
    let until = di.deferred_until.as_deref().unwrap_or("—");
    emit_di_lifecycle(
        db,
        di,
        "di_deferred",
        "di_deferred",
        "warning",
        format!("DI différée — {}", di.code),
        Some(format!("{} différée jusqu'au {until}.", di.title)),
    )
    .await;
}
