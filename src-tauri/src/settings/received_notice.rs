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
//! a random identity and carries a nonce checked after writing, so nothing received can predict,
//! plant or take over the event. Every change applied here gets its own notice: the hook runs only
//! when the received row changes the value held here (a row already applied stops before it), so
//! a site lifted, restored and lifted again at the same recorded instant gives three notices. An error while notifying
//! (only a database failure; the notice is written in its own savepoint, undone on error) depends on
//! the direction of the change:
//! - a change that RELAXES a rule (opt-out on, three-person chain off) fails the apply: the row
//!   becomes a MATERIALIZE_ERROR sync conflict for a person ("take theirs" tries it again; it is not
//!   retried by itself), never applied without its notice;
//! - a change that TIGHTENS a rule (back to four-eyes, three-person chain on) is applied anyway, the
//!   failure logged: the stricter rule is never held back by a notice (fail-safe).
//!
//! If SQLite itself abandoned the whole transaction (disk full, I/O error), undoing the savepoint
//! fails too and the apply ends in error whatever the direction: nothing could be applied in that
//! transaction anyway.
//!
//! The notice cannot be muted or re-routed (`notifications::catalog::ALWAYS_DELIVERED`): it goes to
//! its fixed audience whatever its rule row or preferences say, and nothing of its rule can be edited
//! here. Everything of the category is local to each PC and never touched by sync: its category and
//! rule rows, events, notifications and acknowledgements are kept out of capture (row filters,
//! `sync::row_spec`) and any received upsert or delete of them is refused
//! ([`inbound_local_only_check`]). Each PC that applies a change tells its own administrators, and
//! reading the notice on one PC sends nothing that another PC would wait for. Its category and rule
//! rows are re-created with their fixed values at start-up, after a tenant data reset and before each
//! notice ([`ensure_notice_rows`]), so the inbox always shows it. Delivery ignores preferences on
//! every channel: in-app and desktop toast always, never folded into the daily digest, never emailed
//! (`notifications::os_toast`, `digest`, `email`); its preference rows stay local and received ones are
//! refused. Like any notice it is removed after the plant's notification retention (at least 7 days).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Map, Value};

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit, Arrival, NotificationEventInput, NotificationMessage};
use crate::settings::site::InboundVerdict;

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
    let reason = change_reason(conn, site_sync, new_value, &changed_at).await?;
    let key = if new_value {
        "site_rule_received_on"
    } else {
        "site_rule_received_off"
    };
    let who = by.clone().unwrap_or_else(|| "?".to_string());
    let (title, body) = if new_value {
        (
            format!(
                "Règle des quatre yeux désactivée à {site_label} (autre poste) / Four-eyes lifted at {site_label} \
                 (another PC)"
            ),
            format!(
                "Reçu par synchronisation : {who} a autorisé l'auto-approbation à {site_label} le {changed_at}. \
                 / Received by sync: {who} allowed self-approval at {site_label} on {changed_at}."
            ),
        )
    } else {
        (
            format!(
                "Règle des quatre yeux réactivée à {site_label} (autre poste) / Four-eyes restored at {site_label} \
                 (another PC)"
            ),
            format!(
                "Reçu par synchronisation : {who} a réactivé la règle des quatre yeux à {site_label} le {changed_at}. \
                 / Received by sync: {who} restored four-eyes at {site_label} on {changed_at}."
            ),
        )
    };
    send(
        conn,
        "site_settings",
        entity_sync_id,
        // Back to four-eyes is the stricter direction.
        !new_value,
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

/// The reason of this change: the audit event of the same site, rule and value whose time is the
/// row's `updated_at` (compared as instants: PCs may write them differently). No such event here (an
/// "off" change may arrive before its event, `settings::site`): no reason, never an older one.
async fn change_reason(
    conn: &impl ConnectionTrait,
    site_sync: &str,
    new_value: bool,
    changed_at: &str,
) -> AppResult<Option<String>> {
    let Some(at) = crate::settings::site::instant(changed_at) else {
        return Ok(None);
    };
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.reason AS r, e.changed_at AS t FROM site_setting_events e \
               JOIN org_nodes n ON n.id = e.site_node_id \
              WHERE n.entity_sync_id = ? AND e.setting_key = ? AND TRIM(e.new_value_json) = ?",
            [
                site_sync.into(),
                crate::reliability::study::decisions::SELF_APPROVAL_SETTING.into(),
                (if new_value { "true" } else { "false" }).into(),
            ],
        ))
        .await?;
    for r in rows {
        let t: Option<String> = r.try_get("", "t").map_err(hook_err)?;
        if t.as_deref().and_then(crate::settings::site::instant) == Some(at) {
            return r.try_get::<Option<String>>("", "r").map_err(hook_err);
        }
    }
    Ok(None)
}

/// Everything of the category [`RECEIVED_RULE_CATEGORY`] is local to each PC and sync never touches
/// it: its category and rule rows, its events, their notifications and acknowledgements (row filters
/// keep them out of capture, `sync::row_spec`). A received upsert or delete of any of these rows
/// is refused as a conflict (`NOTIFICATION_CATEGORY_LOCKED`) when the row is of that category here
/// (judged on the LOCAL row by its sync id, so a rename through the sync id is refused too) or the
/// received row names that category (another row may not take its code, an event or notification
/// may not be planted under it). `received`: `None` for a delete.
pub(crate) async fn inbound_local_only_check(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    received: Option<&Payload>,
) -> AppResult<Option<InboundVerdict>> {
    // The category of the row held here, by its sync identity (fixed SQL per table).
    let local_sql = match entity_type {
        "notification_categories" => "SELECT code AS c FROM notification_categories WHERE entity_sync_id = ?",
        "notification_rules" => "SELECT category_code AS c FROM notification_rules WHERE entity_sync_id = ?",
        "notification_events" => "SELECT category_code AS c FROM notification_events WHERE entity_sync_id = ?",
        "notification_preferences" => {
            "SELECT category_code AS c FROM notification_preferences WHERE entity_sync_id = ?"
        }
        "notifications" => {
            "SELECT e.category_code AS c FROM notifications n \
               JOIN notification_events e ON e.id = n.notification_event_id WHERE n.entity_sync_id = ?"
        }
        "notification_acknowledgements" => {
            "SELECT e.category_code AS c FROM notification_acknowledgements a \
               JOIN notifications n ON n.id = a.notification_id \
               JOIN notification_events e ON e.id = n.notification_event_id WHERE a.entity_sync_id = ?"
        }
        _ => return Ok(None),
    };
    let category_of = |sql: &'static str, key: String| async move {
        conn.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [key.into()]))
            .await?
            .map(|r| r.try_get::<Option<String>>("", "c"))
            .transpose()
            .map_err(hook_err)
            .map(Option::flatten)
    };
    let held = category_of(local_sql, entity_sync_id.to_string()).await?;
    // The category the received row names (directly, or through the event / notification it names).
    let named = match received {
        None => None,
        Some(p) => match entity_type {
            "notification_categories" => text(p, "code").map(str::to_string),
            "notification_rules" | "notification_events" | "notification_preferences" => {
                text(p, "category_code").map(str::to_string)
            }
            "notifications" => match text(p, "notification_event_sync_id") {
                Some(event) => {
                    category_of(
                        "SELECT category_code AS c FROM notification_events WHERE entity_sync_id = ?",
                        event.to_string(),
                    )
                    .await?
                }
                None => None,
            },
            _ => match text(p, "notification_sync_id") {
                Some(notification) => {
                    category_of(
                        "SELECT e.category_code AS c FROM notifications n \
                           JOIN notification_events e ON e.id = n.notification_event_id WHERE n.entity_sync_id = ?",
                        notification.to_string(),
                    )
                    .await?
                }
                None => None,
            },
        },
    };
    let locked = |c: &Option<String>| c.as_deref() == Some(RECEIVED_RULE_CATEGORY);
    if locked(&held) || locked(&named) {
        let what = if received.is_none() { "deleted" } else { "changed" };
        return Ok(Some(InboundVerdict::Conflict {
            code: "NOTIFICATION_CATEGORY_LOCKED",
            message: format!(
                "{entity_type} {entity_sync_id}: rows of '{RECEIVED_RULE_CATEGORY}' stay on each PC and are never \
                 {what} by synchronisation"
            ),
        }));
    }
    Ok(None)
}

#[cfg(test)]
thread_local! {
    /// Tests only: the id the next notice event takes (to plant a row under it first).
    pub(crate) static FORCED_EVENT_ID: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// A random id for a notice event (tests may force one, [`FORCED_EVENT_ID`]).
fn new_event_id() -> String {
    #[cfg(test)]
    if let Some(id) = FORCED_EVENT_ID.with(|f| f.borrow_mut().take()) {
        return id;
    }
    crate::sync::identity::new_entity_sync_id()
}

/// The fixed category and rule rows of the notice (migration 246's ids and values), created when
/// missing and put back to their fixed values: run by the startup seed (also after a tenant data
/// reset, which wipes them) and before each notice. They never travel, so nothing else brings them
/// back. Also removes any preference row of the category (ignored anyway). Idempotent.
pub(crate) async fn ensure_notice_rows(conn: &impl ConnectionTrait) -> AppResult<()> {
    let audiences = serde_json::to_string(
        &crate::notifications::catalog::ALWAYS_DELIVERED
            .iter()
            .filter(|(c, _)| *c == RECEIVED_RULE_CATEGORY)
            .map(|(_, a)| *a)
            .collect::<Vec<_>>(),
    )
    .map_err(hook_err)?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_categories \
         (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, module_code, \
          is_offered) VALUES (?, ?, 'Safety rule changed on another PC', 'warning', 0, 0, 'system', 1) \
         ON CONFLICT(code) DO UPDATE SET default_severity = 'warning', default_requires_ack = 0, \
           is_user_configurable = 0, module_code = 'system', is_offered = 1",
        [
            crate::sync::identity::seeded_entity_sync_id("notification_categories", RECEIVED_RULE_CATEGORY).into(),
            RECEIVED_RULE_CATEGORY.into(),
        ],
    ))
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notification_rules SET routing_mode = 'manual', requires_ack = 0, dedupe_window_minutes = 0, \
           escalation_policy_id = NULL, is_active = 1, lead_time_minutes = NULL, audiences_json = ? \
         WHERE category_code = ?",
        [audiences.clone().into(), RECEIVED_RULE_CATEGORY.into()],
    ))
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_rules \
         (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, \
          is_active, lead_time_minutes, audiences_json) \
         SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, ? \
         WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
        [
            crate::sync::identity::seeded_entity_sync_id("notification_rules", RECEIVED_RULE_CATEGORY).into(),
            RECEIVED_RULE_CATEGORY.into(),
            audiences.into(),
            RECEIVED_RULE_CATEGORY.into(),
        ],
    ))
    .await?;
    // Its preferences are ignored on every channel: a row left from an earlier version (or received
    // before such rows were refused) would only show stale toggles. Removed, never sent (row filter).
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM notification_preferences WHERE category_code = ?",
        [RECEIVED_RULE_CATEGORY.into()],
    ))
    .await?;
    Ok(())
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
    send(
        conn,
        "permit_types",
        entity_sync_id,
        // The three-person chain switched on is the stricter direction.
        new_value,
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

/// Writes the notice in a savepoint of the apply. On a (database) error the savepoint is undone; the
/// apply then fails when the change relaxes a rule (never applied without its notice) and goes on,
/// the failure logged, when it `tightens` one (a stricter rule is never held back by a notice).
#[allow(clippy::too_many_arguments)]
async fn send(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    tightens: bool,
    title: String,
    body: String,
    action_url: &str,
    message: NotificationMessage,
) -> AppResult<()> {
    conn.execute_unprepared("SAVEPOINT received_rule_notice").await?;
    let written = emit_notice(conn, entity_type, entity_sync_id, title, body, action_url, &message).await;
    match written {
        Ok(()) => {
            conn.execute_unprepared("RELEASE received_rule_notice").await?;
            Ok(())
        }
        Err(err) => {
            conn.execute_unprepared("ROLLBACK TO received_rule_notice").await?;
            conn.execute_unprepared("RELEASE received_rule_notice").await?;
            if tightens {
                tracing::error!(
                    entity_type,
                    entity_sync_id,
                    error = %err,
                    "received safety rule tightened: applied without its notice (notice failed)"
                );
                Ok(())
            } else {
                Err(err)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn emit_notice(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    title: String,
    body: String,
    action_url: &str,
    message: &NotificationMessage,
) -> AppResult<()> {
    // Every change applied here gets its own notice: the hook runs only when the received row
    // changes the value held here, and a row already applied stops before it (Unchanged). The fixed
    // category and rule rows are made sure of first, so the notice always shows in the inbox.
    ensure_notice_rows(conn).await?;
    // A random event id, and a nonce in the event: after writing, the event must be the one written
    // here (never an existing row adopted under the same id).
    let event_id = new_event_id();
    let nonce = crate::sync::identity::new_entity_sync_id();
    emit(
        conn,
        &NotificationEventInput {
            source_module: "settings".to_string(),
            source_record_id: Some(entity_sync_id.to_string()),
            event_code: RECEIVED_RULE_CATEGORY.to_string(),
            category_code: RECEIVED_RULE_CATEGORY.to_string(),
            severity: "warning".to_string(),
            dedupe_key: None,
            payload_json: Some(
                json!({ "entity_type": entity_type, "entity_sync_id": entity_sync_id, "notice": nonce }).to_string(),
            ),
            title,
            body: Some(body),
            action_url: Some(action_url.to_string()),
        },
        Some(event_id.clone()),
        Some(message),
        Arrival::New,
    )
    .await?;
    let created = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM notification_events WHERE entity_sync_id = ? AND category_code = ? \
               AND json_extract(payload_json, '$.notice') = ?",
            [event_id.into(), RECEIVED_RULE_CATEGORY.into(), nonce.into()],
        ))
        .await?
        .is_some();
    if !created {
        return Err(hook_err(format!(
            "the notice of {entity_type} {entity_sync_id} was not created here"
        )));
    }
    Ok(())
}
