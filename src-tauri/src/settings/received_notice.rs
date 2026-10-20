//! Notice to the settings administrators when a safety rule changed on another PC is applied here
//! (CMMS Advisor decisions A/B on KNOWN_LIMITS rows 30/32/33).
//!
//! Two received changes are announced, in-app, to the users holding `adm.settings` (and the
//! elevated administrators), category [`RECEIVED_RULE_CATEGORY`] (migration 246):
//! - a site's four-eyes opt-out (`site_settings`, `reliability.decisions.allow_self_approval`)
//!   whose value changes here (a site that had no rule here counts as the strict default, off);
//! - a permit type's three-person lock-removal setting (`permit_types.requires_three_person_removal`)
//!   that changes here, or a new type received with it on.
//!
//! The notice names the site or permit type, the new value, who the received record says made the
//! change, when, and why when the record says it. The sending PC is not named: nothing received
//! records it on this PC (the inbox keeps the server batch only); the durable sending-device log is
//! server-side work (`docs/KNOWN_LIMITS.md`).
//!
//! When: from the apply hook (`sync::apply_hooks::before_upsert_checked`), once the row passed its
//! safety check, inside the transaction that applies the row. A row refused or left waiting rolls
//! that transaction back, so no notice is left; a local change never goes through it. The event has
//! a seeded identity built from the row and its recorded change (who, when, value), so applying the
//! same row again (a retry, a second pass, "take theirs") creates no second notice. An error while
//! notifying fails the apply, which is retried like any failed row: a notice is never lost silently.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Map, Value};

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit, Arrival, NotificationEventInput, NotificationMessage};
use crate::sync::identity::seeded_entity_sync_id;

type Payload = Map<String, Value>;

/// Notification category of a safety rule changed on another PC (migration 246).
pub const RECEIVED_RULE_CATEGORY: &str = "safety_rule_received";

fn text<'a>(p: &'a Payload, key: &str) -> Option<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn on(p: &Payload, key: &str) -> bool {
    match p.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        Some(Value::String(s)) => s.trim() == "1" || s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn hook_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("received rule notice: {e}"))
}

/// Display name of the user with this sync id here (`None`: not named, or not on this PC).
async fn person(conn: &impl ConnectionTrait, user_sync_id: Option<&str>) -> AppResult<Option<String>> {
    let Some(sync_id) = user_sync_id else {
        return Ok(None);
    };
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT COALESCE(display_name, username) AS n FROM user_accounts WHERE entity_sync_id = ?",
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<Option<String>>("", "n"))
    .transpose()
    .map_err(hook_err)
    .map(Option::flatten)
}

/// Announces a received row of `entity_type` that passed its safety check, when it changes one of
/// the announced rules here (see the module documentation).
pub(crate) async fn notify_received_rule_change(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<()> {
    match entity_type {
        "site_settings" => site_rule(conn, entity_sync_id, received).await,
        "permit_types" => three_person(conn, entity_sync_id, received).await,
        _ => Ok(()),
    }
}

async fn site_rule(conn: &impl ConnectionTrait, entity_sync_id: &str, received: &Payload) -> AppResult<()> {
    let local = crate::permit::sync_check::local_wire(conn, "site_settings", entity_sync_id).await?;
    let row = crate::permit::sync_check::overlay(local.as_ref(), received);
    if text(&row, "setting_key") != Some(crate::reliability::study::decisions::SELF_APPROVAL_SETTING) {
        return Ok(());
    }
    let value = |p: &Payload| text(p, "value_json").map(|v| v == "true").unwrap_or(false);
    let new_value = value(&row);
    // No rule here: the strict default (four-eyes) was in force.
    if local.as_ref().map_or(false, value) == new_value {
        return Ok(());
    }
    let site_sync = text(&row, "site_node_sync_id").unwrap_or_default();
    let site = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code, name FROM org_nodes WHERE entity_sync_id = ?",
            [site_sync.into()],
        ))
        .await?;
    let (site_code, site_name) = match site {
        Some(r) => (
            r.try_get::<Option<String>>("", "code").map_err(hook_err)?,
            r.try_get::<Option<String>>("", "name").map_err(hook_err)?,
        ),
        None => (None, None),
    };
    let site_label = site_name.or(site_code).unwrap_or_else(|| site_sync.to_string());
    let changed_at = text(&row, "updated_at").unwrap_or_default().to_string();
    let by = person(conn, text(&row, "updated_by_sync_id")).await?;
    // The reason is on the rule's audit event (received with it, `settings::site`).
    let reason = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.reason AS r FROM site_setting_events e JOIN org_nodes n ON n.id = e.site_node_id \
             WHERE n.entity_sync_id = ? AND e.setting_key = ? AND TRIM(e.new_value_json) = ? \
             ORDER BY e.changed_at DESC, e.id DESC LIMIT 1",
            [
                site_sync.into(),
                crate::reliability::study::decisions::SELF_APPROVAL_SETTING.into(),
                (if new_value { "true" } else { "false" }).into(),
            ],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "r"))
        .transpose()
        .map_err(hook_err)?
        .flatten();
    let key = if new_value {
        "site_rule_received_on"
    } else {
        "site_rule_received_off"
    };
    let who = by.clone().unwrap_or_else(|| "?".to_string());
    let (title, body) = if new_value {
        (
            format!("Quatre yeux levé à {site_label} (autre poste) / Four-eyes lifted at {site_label} (another PC)"),
            format!(
                "Reçu par synchronisation : {who} a autorisé l'auto-approbation à {site_label} le {changed_at}. \
                 / Received by sync: {who} allowed self-approval at {site_label} on {changed_at}."
            ),
        )
    } else {
        (
            format!(
                "Quatre yeux rétabli à {site_label} (autre poste) / Four-eyes restored at {site_label} (another PC)"
            ),
            format!(
                "Reçu par synchronisation : {who} a rétabli les quatre yeux à {site_label} le {changed_at}. \
                 / Received by sync: {who} restored four-eyes at {site_label} on {changed_at}."
            ),
        )
    };
    let basis = format!("{RECEIVED_RULE_CATEGORY}|site|{entity_sync_id}|{changed_at}|{new_value}");
    send(
        conn,
        "site_settings",
        entity_sync_id,
        &basis,
        title,
        body,
        "/settings",
        NotificationMessage::new(
            key,
            json!({ "site": site_label, "changed_by": by, "changed_at": changed_at, "reason": reason }),
        ),
    )
    .await
}

async fn three_person(conn: &impl ConnectionTrait, entity_sync_id: &str, received: &Payload) -> AppResult<()> {
    const FLAG: &str = "requires_three_person_removal";
    let local = crate::permit::sync_check::local_wire(conn, "permit_types", entity_sync_id).await?;
    let row = crate::permit::sync_check::overlay(local.as_ref(), received);
    let new_value = on(&row, FLAG);
    let changed = match &local {
        Some(l) => on(l, FLAG) != new_value,
        // A new type: announced when it arrives with the chain on (as its arrival check judges).
        None => new_value,
    };
    if !changed {
        return Ok(());
    }
    let label = text(&row, "name")
        .or_else(|| text(&row, "code"))
        .unwrap_or(entity_sync_id)
        .to_string();
    let changed_at = text(&row, "three_person_changed_at").unwrap_or_default().to_string();
    let by = person(conn, text(&row, "three_person_changed_by_sync_id")).await?;
    let reason = text(&row, "three_person_change_reason").map(str::to_string);
    let who = by.clone().unwrap_or_else(|| "?".to_string());
    let (key, title, body) = if new_value {
        (
            "three_person_received_on",
            format!("Retrait à trois personnes activé : {label} (autre poste) / Three-person removal on: {label} (another PC)"),
            format!(
                "Reçu par synchronisation : {who} a exigé une troisième personne pour le retrait des cadenas ({label}) le \
                 {changed_at}. / Received by sync: {who} required a third person for lock removal ({label}) on {changed_at}."
            ),
        )
    } else {
        (
            "three_person_received_off",
            format!("Retrait à trois personnes désactivé : {label} (autre poste) / Three-person removal off: {label} (another PC)"),
            format!(
                "Reçu par synchronisation : {who} a retiré l'exigence d'une troisième personne ({label}) le {changed_at}. \
                 / Received by sync: {who} removed the third-person requirement ({label}) on {changed_at}."
            ),
        )
    };
    let basis = format!("{RECEIVED_RULE_CATEGORY}|three_person|{entity_sync_id}|{changed_at}|{new_value}");
    send(
        conn,
        "permit_types",
        entity_sync_id,
        &basis,
        title,
        body,
        "/permits",
        NotificationMessage::new(
            key,
            json!({ "permit_type": label, "changed_by": by, "changed_at": changed_at, "reason": reason }),
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    basis: &str,
    title: String,
    body: String,
    action_url: &str,
    message: NotificationMessage,
) -> AppResult<()> {
    emit(
        conn,
        &NotificationEventInput {
            source_module: "settings".to_string(),
            source_record_id: Some(entity_sync_id.to_string()),
            event_code: RECEIVED_RULE_CATEGORY.to_string(),
            category_code: RECEIVED_RULE_CATEGORY.to_string(),
            severity: "warning".to_string(),
            dedupe_key: None,
            payload_json: Some(json!({ "entity_type": entity_type, "entity_sync_id": entity_sync_id }).to_string()),
            title,
            body: Some(body),
            action_url: Some(action_url.to_string()),
        },
        Some(seeded_entity_sync_id("notification_events", basis)),
        Some(&message),
        Arrival::New,
    )
    .await?;
    Ok(())
}
