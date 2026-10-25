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
//! refused.
//!
//! Each administrator keeps their notice until they acknowledge it (CMMS Advisor, like the
//! recovery-code notice): it is shown after each sign-in on this PC ([`notices_for`]), never
//! expired by the notification retention, never archived or snoozed before its acknowledgement, and
//! never deleted (local triggers, [`NOTICE_GUARDS`]; a full tenant data reset of the PC excepted).
//! Acknowledging is the recipient's alone and is audited ([`acknowledge_notice`]).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Map, Value};

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit, Arrival, NotificationEventInput, NotificationMessage};
use crate::settings::site::InboundVerdict;

type Payload = Map<String, Value>;

/// Notification category of a safety rule changed on another PC (migration 246).
pub const RECEIVED_RULE_CATEGORY: &str = "safety_rule_received";

/// Notification category of a lock taken off by a forced removal (migration 249): told to the
/// users holding `ptw.close` at the permit's site, on the PC where it was recorded and on each PC
/// that applies it ([`announce_forced_removal`], [`notify_received_rule_change`]).
pub const FORCED_REMOVAL_CATEGORY: &str = "loto_forced_removal";

/// Notification category of two concurrent versions of a PM plan, or of two occurrences of a plan
/// for the same due date from two such versions (`pm::version_clash`): told to the users holding
/// the PM planning right (`pm.edit`) plant-wide or at the site of the plan's equipment, on each PC
/// that receives the second one, kept until each acknowledges it, like the other locked notices.
pub const PM_CONCURRENT_VERSIONS_CATEGORY: &str = "pm_concurrent_versions";

/// Notification category of a running work order put on hold by the suspension of its permit
/// (owner decision D5): told to the work order's responsible, on the PC where the permit was
/// suspended and on each PC that applies the suspension while holding the work order on hold
/// ([`announce_work_held_by_suspension_of`]).
pub const PERMIT_SUSPENDED_WORK_HELD_CATEGORY: &str = "permit_suspended_work_held";

/// Notification category of a permit closed, cancelled or suspended on another PC before, or while,
/// its work order ran here (owner decision D9, AU-101; `permit::ended_under_work`): told to the work
/// order's responsible and to the permit close-right holders of the permit's site, on each PC that
/// applies it ([`announce_permit_ended_under_work`]).
pub const PERMIT_ENDED_UNDER_WORK_CATEGORY: &str = "permit_ended_under_work";

/// Notification category of a work order received already started without the plant permit rule
/// that concerns it here (owner decision D2 (a), audit finding AU-04 R2-S3;
/// `permit::requirement_rules::finding_for_started_without_rule`): told to the permit-issue right
/// holders (`ptw.issue`) of the work order's site, plant-wide ones included; nobody there: the
/// settings administrators (`notifications::catalog::ALWAYS_DELIVERED_FALLBACK`), on each PC that
/// applies it ([`notify_work_order_without_rule`]).
pub const WO_PERMIT_RULE_MISSING_CATEGORY: &str = "wo_permit_rule_missing";

/// Notification category of an inspection anomaly cancelled after it was routed to a request or a
/// work order (its reading corrected; CMMS Advisor decision on AU-72, option B): told to the
/// follow-up's responsible, on the PC that cancelled it and on each PC that applies the cancel
/// ([`announce_anomaly_origin_cancelled`]).
pub const ANOMALY_ORIGIN_CANCELLED_CATEGORY: &str = "insp_anomaly_origin_cancelled";

/// Every category of a persistent safety notice: kept for each recipient until they acknowledge it,
/// never muted, re-routed, expired, archived or deleted before, and local to each PC. The SQL of
/// the notification modules names them through [`safety_notice_categories_sql`].
pub const SAFETY_NOTICE_CATEGORIES: &[&str] = &[
    RECEIVED_RULE_CATEGORY,
    FORCED_REMOVAL_CATEGORY,
    PM_CONCURRENT_VERSIONS_CATEGORY,
    PERMIT_SUSPENDED_WORK_HELD_CATEGORY,
    PERMIT_ENDED_UNDER_WORK_CATEGORY,
    WO_PERMIT_RULE_MISSING_CATEGORY,
    ANOMALY_ORIGIN_CANCELLED_CATEGORY,
];

/// SQL list of [`SAFETY_NOTICE_CATEGORIES`], for the constant SQL of the notification modules
/// (`concat!`); a test keeps both the same.
#[macro_export]
macro_rules! safety_notice_categories_sql {
    () => {
        "('safety_rule_received', 'loto_forced_removal', 'pm_concurrent_versions', 'permit_suspended_work_held', \
         'permit_ended_under_work', 'wo_permit_rule_missing', 'insp_anomaly_origin_cancelled')"
    };
}

/// Whether `category` is one of [`SAFETY_NOTICE_CATEGORIES`].
pub fn is_safety_notice_category(category: &str) -> bool {
    SAFETY_NOTICE_CATEGORIES.contains(&category)
}

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
        "permit_isolations" => received_forced_removal(conn, entity_sync_id, received).await,
        "inspection_anomalies" => received_anomaly_cancel(conn, entity_sync_id, received).await,
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
    if local.as_ref().is_some_and(value) == new_value {
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
    let locked = |c: &Option<String>| c.as_deref().is_some_and(is_safety_notice_category);
    if locked(&held) || locked(&named) {
        let what = if received.is_none() { "deleted" } else { "changed" };
        let category = held.or(named).unwrap_or_default();
        return Ok(Some(InboundVerdict::Conflict {
            code: "NOTIFICATION_CATEGORY_LOCKED",
            message: format!(
                "{entity_type} {entity_sync_id}: rows of '{category}' stay on each PC and are never \
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
    for (category, label) in [
        (RECEIVED_RULE_CATEGORY, "Safety rule changed on another PC"),
        (FORCED_REMOVAL_CATEGORY, "Lock taken off by a forced removal"),
        (PM_CONCURRENT_VERSIONS_CATEGORY, "Concurrent PM plan versions"),
        (
            PERMIT_SUSPENDED_WORK_HELD_CATEGORY,
            "Work order put on hold by its permit's suspension",
        ),
        (
            PERMIT_ENDED_UNDER_WORK_CATEGORY,
            "Permit ended on another PC before or during the work",
        ),
        (
            WO_PERMIT_RULE_MISSING_CATEGORY,
            "Work order received started without its plant permit rule",
        ),
        (
            ANOMALY_ORIGIN_CANCELLED_CATEGORY,
            "Originating inspection anomaly cancelled",
        ),
    ] {
        ensure_category_rows(conn, category, label).await?;
    }
    // The guards are made again each time: a PC upgraded from an earlier version holds them for the
    // first category only (`CREATE TRIGGER IF NOT EXISTS` would keep that version).
    drop_notice_guards(conn).await?;
    for (_, sql) in NOTICE_GUARDS {
        conn.execute_unprepared(sql).await?;
    }
    // Its preferences are ignored on every channel: a row left from an earlier version (or received
    // before such rows were refused) would only show stale toggles. Removed, never sent (row filter).
    conn.execute_unprepared(concat!(
        "DELETE FROM notification_preferences WHERE category_code IN ",
        crate::safety_notice_categories_sql!()
    ))
    .await?;
    Ok(())
}

/// The fixed category and rule rows of one safety notice category, with its fixed audience.
async fn ensure_category_rows(conn: &impl ConnectionTrait, category: &str, label: &str) -> AppResult<()> {
    let audiences = serde_json::to_string(
        &crate::notifications::catalog::ALWAYS_DELIVERED
            .iter()
            .filter(|(c, _)| *c == category)
            .map(|(_, a)| *a)
            .collect::<Vec<_>>(),
    )
    .map_err(hook_err)?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_categories \
         (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, module_code, \
          is_offered) VALUES (?, ?, ?, 'warning', 0, 0, 'system', 1) \
         ON CONFLICT(code) DO UPDATE SET default_severity = 'warning', default_requires_ack = 0, \
           is_user_configurable = 0, module_code = 'system', is_offered = 1",
        [
            crate::sync::identity::seeded_entity_sync_id("notification_categories", category).into(),
            category.into(),
            label.into(),
        ],
    ))
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notification_rules SET routing_mode = 'manual', requires_ack = 0, dedupe_window_minutes = 0, \
           escalation_policy_id = NULL, is_active = 1, lead_time_minutes = NULL, audiences_json = ? \
         WHERE category_code = ?",
        [audiences.clone().into(), category.into()],
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
            crate::sync::identity::seeded_entity_sync_id("notification_rules", category).into(),
            category.into(),
            audiences.into(),
            category.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Local triggers keeping every notice of the category: its events, notifications and
/// acknowledgements are never deleted (CMMS Advisor: kept for each administrator until
/// acknowledged). Created by [`ensure_notice_rows`] at start-up and before each notice; dropped only
/// by a full tenant data reset of the PC, whose reseed creates them again ([`drop_notice_guards`]).
///
/// For migration authors: these triggers exist when the next upgrade's migrations run. A migration
/// that deletes rows of this category, or rebuilds `notification_events`, `notifications` or
/// `notification_acknowledgements` (create, copy, drop, rename), must call [`drop_notice_guards`]
/// first; the start-up seed creates them again.
const NOTICE_GUARDS: &[(&str, &str)] = &[
    (
        "safety_notice_event_kept",
        concat!(
            "CREATE TRIGGER IF NOT EXISTS safety_notice_event_kept BEFORE DELETE ON notification_events \
             WHEN OLD.category_code IN ",
            crate::safety_notice_categories_sql!(),
            " BEGIN SELECT RAISE(ABORT, 'NOTIFICATION_CATEGORY_LOCKED: a safety notice is never deleted'); END"
        ),
    ),
    (
        "safety_notice_kept",
        concat!(
            "CREATE TRIGGER IF NOT EXISTS safety_notice_kept BEFORE DELETE ON notifications \
             WHEN EXISTS (SELECT 1 FROM notification_events e WHERE e.id = OLD.notification_event_id \
                            AND e.category_code IN ",
            crate::safety_notice_categories_sql!(),
            ") BEGIN SELECT RAISE(ABORT, 'NOTIFICATION_CATEGORY_LOCKED: a safety notice is never deleted'); END"
        ),
    ),
    (
        "safety_notice_ack_kept",
        concat!(
            "CREATE TRIGGER IF NOT EXISTS safety_notice_ack_kept BEFORE DELETE ON notification_acknowledgements \
             WHEN EXISTS (SELECT 1 FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
                            WHERE n.id = OLD.notification_id AND e.category_code IN ",
            crate::safety_notice_categories_sql!(),
            ") BEGIN SELECT RAISE(ABORT, 'NOTIFICATION_CATEGORY_LOCKED: an acknowledged safety notice is kept'); END"
        ),
    ),
];

/// Drops the guards before a full tenant data reset wipes the PC's data (its reseed runs
/// [`ensure_notice_rows`], which creates them again).
pub(crate) async fn drop_notice_guards(conn: &impl ConnectionTrait) -> AppResult<()> {
    for (name, _) in NOTICE_GUARDS {
        conn.execute_unprepared(&format!("DROP TRIGGER IF EXISTS {name}"))
            .await?;
    }
    Ok(())
}

/// A safety rule notice of the signed-in administrator still to acknowledge (shown after each
/// sign-in until acknowledged, as the recovery-code notice is).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SafetyRuleNotice {
    /// The notification's local id (what [`acknowledge_notice`] takes).
    pub id: i64,
    pub created_at: String,
    pub title: String,
    pub body: Option<String>,
    pub message_key: Option<String>,
    pub message_params: Option<serde_json::Value>,
}

/// The notices of `user_id` not acknowledged yet, oldest first, while that account is active.
/// Never expired, archived or snoozed away before acknowledgement (they stay here until then).
pub(crate) async fn notices_for(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<Vec<SafetyRuleNotice>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.id, n.created_at, n.title, n.body, e.message_key, e.message_params_json \
               FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
               JOIN user_accounts u ON u.id = n.recipient_user_id \
              WHERE n.recipient_user_id = ? AND e.category_code IN (SELECT value FROM json_each(?)) \
                AND n.acknowledged_at IS NULL AND u.is_active = 1 AND u.deleted_at IS NULL \
              ORDER BY n.created_at, n.id",
            [user_id.into(), categories_json().into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let params: Option<String> = r.try_get("", "message_params_json").map_err(hook_err)?;
            Ok(SafetyRuleNotice {
                id: r.try_get("", "id").map_err(hook_err)?,
                created_at: r.try_get("", "created_at").map_err(hook_err)?,
                title: r.try_get("", "title").map_err(hook_err)?,
                body: r.try_get("", "body").map_err(hook_err)?,
                message_key: r.try_get("", "message_key").map_err(hook_err)?,
                message_params: params.and_then(|p| serde_json::from_str(&p).ok()),
            })
        })
        .collect()
}

/// [`SAFETY_NOTICE_CATEGORIES`] as a JSON array (bound to `json_each`).
fn categories_json() -> String {
    serde_json::to_string(SAFETY_NOTICE_CATEGORIES).unwrap_or_else(|_| "[]".to_string())
}

/// Whether notification `notification_id` is a safety rule notice not acknowledged yet.
pub(crate) async fn is_unacknowledged_notice(conn: &impl ConnectionTrait, notification_id: i64) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
              WHERE n.id = ? AND e.category_code IN (SELECT value FROM json_each(?)) AND n.acknowledged_at IS NULL",
            [notification_id.into(), categories_json().into()],
        ))
        .await?
        .is_some())
}

/// Whether notification `notification_id` is a safety rule notice (acknowledged or not).
pub(crate) async fn is_notice(conn: &impl ConnectionTrait, notification_id: i64) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
              WHERE n.id = ? AND e.category_code IN (SELECT value FROM json_each(?))",
            [notification_id.into(), categories_json().into()],
        ))
        .await?
        .is_some())
}

/// Acknowledges a safety rule notice: only by its recipient, once. Recorded (who, when) and audited
/// (`notification.safety_notice_acknowledged`: who, when, which notice and its change) in one
/// transaction; the acknowledgement stays on this PC (row filter).
pub(crate) async fn acknowledge_notice(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
    notification_id: i64,
) -> AppResult<()> {
    use sea_orm::TransactionTrait;
    // Holding the write lock from the start (as the recovery-code acknowledgement): a second click
    // waits, then finds the notice acknowledged.
    let tx = db.begin().await?;
    tx.execute_unprepared("UPDATE notifications SET id = id WHERE 0")
        .await?;
    let row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.entity_sync_id AS event, e.message_key AS k, e.message_params_json AS p, \
                    e.category_code AS c \
               FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
              WHERE n.id = ? AND n.recipient_user_id = ? AND e.category_code IN (SELECT value FROM json_each(?)) \
                AND n.acknowledged_at IS NULL",
            [notification_id.into(), user_id.into(), categories_json().into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_NOTIFICATION_ACK_NOT_YOURS: this safety notice is not yours or was already acknowledged."
                    .to_string(),
            ])
        })?;
    let event: Option<String> = row.try_get("", "event").map_err(hook_err)?;
    let key: Option<String> = row.try_get("", "k").map_err(hook_err)?;
    let params: Option<String> = row.try_get("", "p").map_err(hook_err)?;
    let category: String = row.try_get("", "c").map_err(hook_err)?;
    crate::notifications::delivery::record_acknowledgement(&tx, notification_id, user_id, None).await?;
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: AUDIT_NOTICE_ACKNOWLEDGED.to_string(),
            target_type: Some("notification".to_string()),
            target_id: Some(notification_id.to_string()),
            actor_id: Some(user_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "security".to_string(),
            details_json: Some(json!({
                "category": category,
                "notification_id": notification_id,
                "event_sync_id": event,
                "message_key": key,
                "message_params": params.and_then(|p| serde_json::from_str::<Value>(&p).ok()),
                "acknowledged_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            })),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Audit action of an acknowledged safety rule notice.
pub const AUDIT_NOTICE_ACKNOWLEDGED: &str = "notification.safety_notice_acknowledged";

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
    let route = NoticeRoute {
        category: RECEIVED_RULE_CATEGORY,
        source_module: "settings",
        source_record_id: entity_sync_id.to_string(),
        entity_type,
        entity_sync_id,
        org_node_id: None,
    };
    let written = emit_notice(conn, &route, title, body, action_url, &message).await;
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

/// Writes the review notice of grants made by someone whose right to give access was withdrawn on
/// another PC (`sync::grant_check::after_rights_change`), to the administrators (the audience of
/// [`RECEIVED_RULE_CATEGORY`]: `adm.settings` holders and plant administrators, kept until each
/// acknowledges it), in a savepoint of the apply of the received row. `fail_closed`: a notice that
/// cannot be written fails the caller (a grant applied for review is never applied untold);
/// otherwise it is logged (a withdrawal only takes access away and is never held back by its
/// notice).
pub(crate) async fn send_grant_review_notice(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    title: String,
    body: String,
    message: NotificationMessage,
    fail_closed: bool,
) -> AppResult<()> {
    send(
        conn,
        entity_type,
        entity_sync_id,
        !fail_closed,
        title,
        body,
        "/admin",
        message,
    )
    .await
}

/// A work order received already started without the plant rule that concerns it here
/// (`permit::requirement_rules::finding_for_started_without_rule`): the permit-issue right holders
/// of its site are told, each until they acknowledge it ([`WO_PERMIT_RULE_MISSING_CATEGORY`],
/// routed by the work order's site; nobody there: the settings administrators). The work order is
/// applied whatever happens to the notice (its history is never held back): a notice that cannot be
/// written is logged. The text does not say why the rule was missing there (a PC without the rule
/// list, or an edited row): it says what is known.
pub(crate) async fn notify_work_order_without_rule(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    wo_sync_id: &str,
    wo_code: &str,
    rule: &str,
    permit_type: &str,
    requires_permit: bool,
    received: Option<(&str, Option<&str>)>,
) -> AppResult<()> {
    let (required_fr, required_en) = if requires_permit {
        ("avec un permis exigé", "with a permit required")
    } else {
        ("sans permis exigé", "with no permit required")
    };
    // Received with a rule, but not the one (or not the permit type) the plant applies here: both
    // are named (audit finding AU-04 R5-S2), never "without the rule".
    let (title, body, key) = match received {
        Some((named, named_type)) => {
            let named_type = named_type.unwrap_or("?");
            (
                format!(
                    "OT {wo_code} reçu démarré avec la règle {named} (permis {named_type}) au lieu de {rule} (permis \
                     {permit_type}) / Work order {wo_code} received started with rule {named} ({named_type} permit) \
                     instead of {rule} ({permit_type} permit)"
                ),
                format!(
                    "Reçu par synchronisation : l'OT {wo_code} a été reçu démarré {required_fr}, avec la règle {named} \
                     (permis {named_type}), alors que l'usine applique ici la règle {rule} (permis {permit_type}) pour \
                     son équipement et son travail. Vérifiez comment ce travail a été protégé. / Received by sync: \
                     work order {wo_code} was received started {required_en}, with rule {named} ({named_type} permit), \
                     while the plant applies rule {rule} ({permit_type} permit) here for its equipment and work. Check \
                     how this work was protected."
                ),
                "wo_permit_rule_other_on_arrival",
            )
        }
        None => (
            format!(
                "OT {wo_code} reçu démarré sans la règle de permis {rule} / Work order {wo_code} received started \
                 without permit rule {rule}"
            ),
            format!(
                "Reçu par synchronisation : l'OT {wo_code} a été reçu démarré {required_fr}, sans la règle {rule} que \
                 l'usine applique ici (un permis {permit_type} pour son équipement et son travail). Vérifiez comment ce \
                 travail a été protégé. / Received by sync: work order {wo_code} was received started {required_en}, \
                 without plant rule {rule} that applies here (a {permit_type} permit for its equipment and work). \
                 Check how this work was protected."
            ),
            "wo_permit_rule_missing_on_arrival",
        ),
    };
    let route = NoticeRoute {
        category: WO_PERMIT_RULE_MISSING_CATEGORY,
        // The work order's site routes the permit-issue right holders (`notifications::router`).
        source_module: "work_orders",
        source_record_id: wo_id.to_string(),
        entity_type: "work_orders",
        entity_sync_id: wo_sync_id,
        org_node_id: None,
    };
    let message = NotificationMessage::new(
        key,
        json!({
            "code": wo_code,
            "rule": rule,
            "permit_type": permit_type,
            "requires_permit": requires_permit,
            "received_rule": received.map(|r| r.0),
            "received_permit_type": received.and_then(|r| r.1),
        }),
    );
    send_logged_notice(
        conn,
        &route,
        title,
        body,
        &format!("/work-orders?openWo={wo_id}"),
        &message,
    )
    .await;
    Ok(())
}

/// The permit type of the plant rule held on a work order changed (the list here gives another rule
/// or type, `permit::requirement_rules::converge_received_rule`): the permit-issue right holders of
/// its site are told, each until they acknowledge it ([`WO_PERMIT_RULE_MISSING_CATEGORY`], its
/// audience and fallback), with the rule and permit type before and after (CMMS Advisor decision on
/// AU-04 R5-S3, option A). The change itself is recorded in the work order's history and audit
/// trail; a notice that cannot be written is logged.
pub(crate) async fn notify_work_order_rule_type_changed(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    wo_sync_id: &str,
    wo_code: &str,
    before: (&str, Option<&str>),
    after: (&str, Option<&str>),
) {
    let (old_rule, old_type) = (before.0, before.1.unwrap_or("?"));
    let (new_rule, new_type) = (after.0, after.1.unwrap_or("?"));
    let route = NoticeRoute {
        category: WO_PERMIT_RULE_MISSING_CATEGORY,
        source_module: "work_orders",
        source_record_id: wo_id.to_string(),
        entity_type: "work_orders",
        entity_sync_id: wo_sync_id,
        org_node_id: None,
    };
    send_logged_notice(
        conn,
        &route,
        format!(
            "OT {wo_code} : permis exigé changé de {old_type} à {new_type} / Work order {wo_code}: required permit \
             changed from {old_type} to {new_type}"
        ),
        format!(
            "Les règles de permis de l'usine appliquées ici donnent à l'OT {wo_code} la règle {new_rule} (permis \
             {new_type}) au lieu de {old_rule} (permis {old_type}). Vérifiez le permis prévu pour ce travail. / The \
             plant's permit rules applied here give work order {wo_code} rule {new_rule} ({new_type} permit) instead \
             of {old_rule} ({old_type} permit). Check the permit planned for this work."
        ),
        &format!("/work-orders?openWo={wo_id}"),
        &NotificationMessage::new(
            "wo_permit_rule_type_changed",
            json!({
                "code": wo_code,
                "rule": new_rule,
                "permit_type": new_type,
                "previous_rule": old_rule,
                "previous_permit_type": old_type,
            }),
        ),
    )
    .await;
}

/// Writes a notice in a savepoint of the caller's transaction; a notice that cannot be written is
/// logged and the change it tells of is kept (it is never held back by its notice).
async fn send_logged_notice(
    conn: &impl ConnectionTrait,
    route: &NoticeRoute<'_>,
    title: String,
    body: String,
    action_url: &str,
    message: &NotificationMessage,
) {
    let written = async {
        conn.execute_unprepared("SAVEPOINT safety_notice").await?;
        match emit_notice(conn, route, title, body, action_url, message).await {
            Ok(()) => {
                conn.execute_unprepared("RELEASE safety_notice").await?;
                Ok(())
            }
            Err(err) => {
                conn.execute_unprepared("ROLLBACK TO safety_notice").await?;
                conn.execute_unprepared("RELEASE safety_notice").await?;
                Err(err)
            }
        }
    }
    .await;
    if let Err(err) = written {
        tracing::error!(
            category = route.category,
            entity_type = route.entity_type,
            entity_sync_id = route.entity_sync_id,
            error = %err,
            "safety notice not written; the change it tells of is kept"
        );
    }
}

/// An inspection anomaly routed to a request or a work order whose condition is gone (its reading
/// corrected or its photo added: cancelled by `inspection::results::sync_auto_anomaly`, here or on
/// the PC that cancelled it): its follow-up is left as it is, and the follow-up's responsible is told,
/// until they acknowledge it ([`ANOMALY_ORIGIN_CANCELLED_CATEGORY`]: the work order's responsible, or
/// the responsible of the work order a request was converted to; nobody: the work order's planner,
/// or the request's reviewer). A "revue permis requise" on the work order stays: it is cleared only
/// through its own path. `anomaly_id` is the anomaly's local id. A notice that cannot be written is
/// logged.
pub(crate) async fn announce_anomaly_origin_cancelled(conn: &impl ConnectionTrait, anomaly_id: i64) {
    let found = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.entity_sync_id, a.description, a.linked_di_id, a.linked_work_order_id, \
                    wo.code AS wo_code, di.code AS di_code \
               FROM inspection_anomalies a \
               LEFT JOIN work_orders wo ON wo.id = a.linked_work_order_id \
               LEFT JOIN intervention_requests di ON di.id = a.linked_di_id \
              WHERE a.id = ?",
            [anomaly_id.into()],
        ))
        .await;
    let row = match found {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(err) => {
            tracing::error!(anomaly_id, error = %err, "cancelled anomaly notice: anomaly not read");
            return;
        }
    };
    let get = |c: &str| row.try_get::<Option<String>>("", c).ok().flatten();
    let id = |c: &str| row.try_get::<Option<i64>>("", c).ok().flatten();
    let anomaly_sync = get("entity_sync_id").unwrap_or_default();
    let description = get("description").unwrap_or_default();
    let (module, record, kind, code, action_url) = match (id("linked_work_order_id"), id("linked_di_id")) {
        (Some(wo), _) => (
            "work_orders",
            wo,
            "wo",
            get("wo_code").unwrap_or_else(|| wo.to_string()),
            format!("/work-orders?openWo={wo}"),
        ),
        (None, Some(di)) => (
            "di",
            di,
            "di",
            get("di_code").unwrap_or_else(|| di.to_string()),
            format!("/requests?openDi={di}"),
        ),
        (None, None) => return,
    };
    let route = NoticeRoute {
        category: ANOMALY_ORIGIN_CANCELLED_CATEGORY,
        // The follow-up routes its responsible, planner or reviewer (`notifications::router`).
        source_module: module,
        source_record_id: record.to_string(),
        entity_type: "inspection_anomalies",
        entity_sync_id: &anomaly_sync,
        org_node_id: None,
    };
    send_logged_notice(
        conn,
        &route,
        format!("Anomalie d'origine annulée : {code} / Originating anomaly cancelled: {code}"),
        format!(
            "L'anomalie d'inspection à l'origine de {code} a été annulée : son défaut n'est plus détecté ({description}). \
             {code} n'est pas modifié : vérifiez s'il reste nécessaire. Une revue permis requise reste en place. / \
             The inspection anomaly {code} was raised from was cancelled: its condition is no longer detected ({description}). \
             {code} is not changed: check whether it is still needed. A required permit review stays in place."
        ),
        &action_url,
        &NotificationMessage::new(
            ANOMALY_ORIGIN_CANCELLED_CATEGORY,
            json!({ "code": code, "kind": kind, "anomaly": description }),
        ),
    )
    .await;
}

/// A received cancel of an anomaly routed here (`inspection::sync_check` accepted it): its
/// follow-up's responsible here is told, as on the PC that cancelled it.
async fn received_anomaly_cancel(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<()> {
    let Some(local) = crate::permit::sync_check::local_wire(conn, "inspection_anomalies", entity_sync_id).await? else {
        return Ok(());
    };
    let row = crate::permit::sync_check::overlay(Some(&local), received);
    if text(&local, "resolution_status") != Some("triaged") || text(&row, "resolution_status") != Some("cancelled") {
        return Ok(());
    }
    let id = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_anomalies WHERE entity_sync_id = ?",
            [entity_sync_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(hook_err)?;
    if let Some(id) = id {
        announce_anomaly_origin_cancelled(conn, id).await;
    }
    Ok(())
}

/// Writes a notice of concurrent PM plan versions (`pm::version_clash`) in a savepoint of the apply
/// of the received row. A notice that cannot be written is logged and the row still applies: a
/// version or an occurrence is never held back by its notice (nothing is changed by either).
pub(crate) async fn send_pm_concurrent_notice(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    plan_sync_id: &str,
    site: Option<i64>,
    title: String,
    body: String,
    message: NotificationMessage,
) -> AppResult<()> {
    conn.execute_unprepared("SAVEPOINT pm_concurrent_notice").await?;
    let route = NoticeRoute {
        category: PM_CONCURRENT_VERSIONS_CATEGORY,
        // The plant-wide PM planners, and those of the site of the plan's equipment.
        source_module: "pm_plan",
        source_record_id: plan_sync_id.to_string(),
        entity_type,
        entity_sync_id,
        org_node_id: site,
    };
    match emit_notice(conn, &route, title, body, "/pm", &message).await {
        Ok(()) => conn.execute_unprepared("RELEASE pm_concurrent_notice").await?,
        Err(err) => {
            conn.execute_unprepared("ROLLBACK TO pm_concurrent_notice").await?;
            conn.execute_unprepared("RELEASE pm_concurrent_notice").await?;
            tracing::error!(
                entity_type,
                entity_sync_id,
                error = %err,
                "concurrent PM versions: applied without its notice (notice failed)"
            );
            return Ok(());
        }
    };
    Ok(())
}

/// Where a notice goes: its category, the record it is about (its routing context: a permit routes
/// to its site) and the row that caused it.
struct NoticeRoute<'a> {
    category: &'static str,
    source_module: &'static str,
    source_record_id: String,
    entity_type: &'a str,
    entity_sync_id: &'a str,
    /// The site the notice is about, when its record gives none: its holders at that site are told
    /// too, besides the plant-wide ones.
    org_node_id: Option<i64>,
}

async fn emit_notice(
    conn: &impl ConnectionTrait,
    route: &NoticeRoute<'_>,
    title: String,
    body: String,
    action_url: &str,
    message: &NotificationMessage,
) -> AppResult<()> {
    let (entity_type, entity_sync_id) = (route.entity_type, route.entity_sync_id);
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
            source_module: route.source_module.to_string(),
            source_record_id: Some(route.source_record_id.clone()),
            event_code: route.category.to_string(),
            category_code: route.category.to_string(),
            severity: "warning".to_string(),
            dedupe_key: None,
            payload_json: Some(
                json!({
                    "entity_type": entity_type,
                    "entity_sync_id": entity_sync_id,
                    "notice": nonce,
                    "org_node_id": route.org_node_id,
                })
                .to_string(),
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
            [event_id.into(), route.category.into(), nonce.into()],
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

/// The people and texts of a forced removal, for its notice.
struct ForcedRemovalNotice {
    permit_id: i64,
    permit_code: String,
    point: String,
    worker: Option<String>,
    remover: Option<String>,
    authoriser: Option<String>,
    removed_at: String,
    reason: String,
}

/// Writes the notice of a forced removal to the `ptw.close` holders of the permit's site, in a
/// savepoint: an error fails the caller (a forced removal is never recorded or applied without its
/// notice).
async fn send_forced_removal(
    conn: &impl ConnectionTrait,
    isolation_sync_id: &str,
    n: ForcedRemovalNotice,
) -> AppResult<()> {
    let who = |p: &Option<String>| p.clone().unwrap_or_else(|| "?".to_string());
    let (worker, remover, authoriser) = (who(&n.worker), who(&n.remover), who(&n.authoriser));
    let (permit, point, removed_at, reason) = (&n.permit_code, &n.point, &n.removed_at, &n.reason);
    let title = format!("Retrait forcé d'un cadenas : {point} ({permit}) / Forced lock removal: {point} ({permit})");
    let body = format!(
        "{authoriser} a autorisé le retrait du cadenas de {worker} (absent) par {remover} le {removed_at}. Motif : \
         {reason}. Le travailleur doit en être informé. / {authoriser} authorised {remover} to take off the lock of \
         {worker} (absent) on {removed_at}. Reason: {reason}. The worker must be told."
    );
    let message = NotificationMessage::new(
        FORCED_REMOVAL_CATEGORY,
        json!({
            "permit": permit,
            "point": point,
            "worker": n.worker,
            "removed_by": n.remover,
            "authorised_by": n.authoriser,
            "removed_at": removed_at,
            "reason": reason,
        }),
    );
    let route = NoticeRoute {
        category: FORCED_REMOVAL_CATEGORY,
        source_module: "permit",
        source_record_id: n.permit_id.to_string(),
        entity_type: "permit_isolations",
        entity_sync_id: isolation_sync_id,
        org_node_id: None,
    };
    conn.execute_unprepared("SAVEPOINT forced_removal_notice").await?;
    match emit_notice(conn, &route, title, body, "/permits", &message).await {
        Ok(()) => {
            conn.execute_unprepared("RELEASE forced_removal_notice").await?;
            Ok(())
        }
        Err(err) => {
            conn.execute_unprepared("ROLLBACK TO forced_removal_notice").await?;
            conn.execute_unprepared("RELEASE forced_removal_notice").await?;
            Err(err)
        }
    }
}

/// Display name of the user with this local id (`None`: none, or not on this PC).
async fn person_by_id(conn: &impl ConnectionTrait, user_id: Option<i64>) -> AppResult<Option<String>> {
    let Some(id) = user_id else {
        return Ok(None);
    };
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT COALESCE(display_name, username) AS n FROM user_accounts WHERE id = ?",
        [id.into()],
    ))
    .await?
    .map(|r| r.try_get::<Option<String>>("", "n"))
    .transpose()
    .map_err(hook_err)
    .map(Option::flatten)
}

/// The notice of a forced removal recorded on this PC (`permit::queries::force_permit_isolation_removal`),
/// in its transaction: to the users holding `ptw.close` at the permit's site, kept for each until
/// they acknowledge it.
pub(crate) async fn announce_forced_removal(
    conn: &impl ConnectionTrait,
    permit: &crate::permit::domain::WorkPermit,
    iso: &crate::permit::domain::PermitIsolation,
) -> AppResult<()> {
    let notice = ForcedRemovalNotice {
        permit_id: permit.id,
        permit_code: permit.code.clone(),
        point: iso.isolation_point.clone(),
        worker: match iso.applied_by_name.clone() {
            Some(n) => Some(n),
            None => person_by_id(conn, iso.applied_by_id).await?,
        },
        remover: iso.removed_by_name.clone(),
        authoriser: iso.forced_authorized_by_name.clone(),
        removed_at: iso.removed_at.clone().unwrap_or_default(),
        reason: iso.forced_reason.clone().unwrap_or_default(),
    };
    send_forced_removal(conn, &iso.entity_sync_id, notice).await
}

/// A forced removal received by sync that passed its checks (`permit::sync_check`) and is new here
/// (the copy held here was not forced): the `ptw.close` holders of the permit's site on this PC are
/// told, in the apply's transaction. A permit not here yet: the row waits anyway (nothing told).
async fn received_forced_removal(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<()> {
    let local = crate::permit::sync_check::local_wire(conn, "permit_isolations", entity_sync_id).await?;
    let row = crate::permit::sync_check::overlay(local.as_ref(), received);
    if !on(&row, "forced_removal") || local.as_ref().is_some_and(|l| on(l, "forced_removal")) {
        return Ok(());
    }
    let Some(permit_sync) = text(&row, "permit_sync_id") else {
        return Ok(());
    };
    let Some(permit) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code FROM work_permits WHERE entity_sync_id = ?",
            [permit_sync.into()],
        ))
        .await?
    else {
        return Ok(());
    };
    let notice = ForcedRemovalNotice {
        permit_id: permit.try_get("", "id").map_err(hook_err)?,
        permit_code: permit.try_get("", "code").map_err(hook_err)?,
        point: text(&row, "isolation_point").unwrap_or_default().to_string(),
        worker: person(conn, text(&row, "applied_by_sync_id")).await?,
        remover: person(conn, text(&row, "removed_by_sync_id")).await?,
        authoriser: person(conn, text(&row, "forced_authorized_by_sync_id")).await?,
        removed_at: text(&row, "removed_at").unwrap_or_default().to_string(),
        reason: text(&row, "forced_reason").unwrap_or_default().to_string(),
    };
    send_forced_removal(conn, entity_sync_id, notice).await
}

/// Writes a notice of a permit's work in a savepoint, logged when it cannot be written: the change it
/// tells of (a suspension that stops the work, a received permit end) is never held back by its
/// notice — it makes the plant safer, and a received end is a fact (its review conflict is kept).
async fn send_permit_work_notice(
    conn: &impl ConnectionTrait,
    route: &NoticeRoute<'_>,
    title: String,
    body: String,
    message: NotificationMessage,
) {
    let written = async {
        conn.execute_unprepared("SAVEPOINT permit_work_notice").await?;
        match emit_notice(conn, route, title, body, "/permits", &message).await {
            Ok(()) => {
                conn.execute_unprepared("RELEASE permit_work_notice").await?;
                Ok(())
            }
            Err(err) => {
                conn.execute_unprepared("ROLLBACK TO permit_work_notice").await?;
                conn.execute_unprepared("RELEASE permit_work_notice").await?;
                Err(err)
            }
        }
    }
    .await;
    if let Err(err) = written {
        tracing::error!(
            category = route.category,
            entity_type = route.entity_type,
            entity_sync_id = route.entity_sync_id,
            error = %err,
            "permit work notice not written; the change it tells of is kept"
        );
    }
}

/// A running work order put on hold because its permit was suspended (owner decision D5), in the
/// transaction of the suspension (or of its apply here): its responsible is told, and kept told
/// until they acknowledge it; a work order without a responsible (or whose responsible is not an
/// active user here) tells the permit close-right holders of the permit's site instead
/// (`notifications::catalog::ALWAYS_DELIVERED_FALLBACK`). `permit_id` is the permit's local id (its
/// work order and site route the notice).
pub(crate) async fn announce_work_held_by_suspension_of(
    conn: &impl ConnectionTrait,
    permit_id: i64,
    permit_sync_id: &str,
    permit_code: &str,
    wo_code: &str,
    reason: &str,
) {
    let route = NoticeRoute {
        category: PERMIT_SUSPENDED_WORK_HELD_CATEGORY,
        source_module: "permit",
        source_record_id: permit_id.to_string(),
        entity_type: "work_permits",
        entity_sync_id: permit_sync_id,
        org_node_id: None,
    };
    send_permit_work_notice(
        conn,
        &route,
        format!(
            "OT {wo_code} mis en attente : permis {permit_code} suspendu / Work order {wo_code} on hold: permit \
             {permit_code} suspended"
        ),
        format!(
            "Le permis {permit_code} a été suspendu (motif : {reason}). Le travail de l'OT {wo_code} est arrêté et ne \
             reprendra qu'après la remise en vigueur du permis. / Permit {permit_code} was suspended (reason: \
             {reason}). Work on {wo_code} is stopped until the permit is reinstated."
        ),
        NotificationMessage::new(
            PERMIT_SUSPENDED_WORK_HELD_CATEGORY,
            json!({ "permit": permit_code, "work_order": wo_code, "reason": reason }),
        ),
    )
    .await;
}

/// [`announce_work_held_by_suspension_of`] for a suspension made on this PC.
pub(crate) async fn announce_work_held_by_suspension(
    conn: &impl ConnectionTrait,
    permit: &crate::permit::domain::WorkPermit,
    wo_code: &str,
    reason: &str,
) {
    announce_work_held_by_suspension_of(conn, permit.id, &permit.entity_sync_id, &permit.code, wo_code, reason).await;
}

/// What a received permit end raised for review (`permit::ended_under_work`).
pub(crate) struct PermitEndedNotice<'a> {
    pub permit_id: i64,
    pub permit_sync_id: &'a str,
    pub permit_code: &'a str,
    pub wo_code: &'a str,
    /// The message key (`permit::ended_under_work::codes::NOTICE_*`): ended before the start or
    /// during the work, each with or without the time of the end; or suspended around a start.
    pub key: &'static str,
    /// The permit's status received (`closed`, `handed_back`, `cancelled`, `suspended`,
    /// `revalidation_required`).
    pub status: &'a str,
    /// When the permit ended there, when the row says it (`None`: a cancellation, no time).
    pub end_at: Option<&'a str>,
    /// When a suspension made and lifted there was lifted (`permit_suspended_around_start`).
    pub until: Option<&'a str>,
    /// When the work last started (or resumed, or was reopened) here.
    pub start_at: Option<&'a str>,
}

/// A permit ended on another PC before, or while, its work order ran here (owner decision D9):
/// the work order's responsible and the permit close-right holders of the permit's site are told,
/// in the apply's transaction, each until they acknowledge it.
pub(crate) async fn announce_permit_ended_under_work(conn: &impl ConnectionTrait, n: &PermitEndedNotice<'_>) {
    let route = NoticeRoute {
        category: PERMIT_ENDED_UNDER_WORK_CATEGORY,
        source_module: "permit",
        source_record_id: n.permit_id.to_string(),
        entity_type: "work_permits",
        entity_sync_id: n.permit_sync_id,
        org_node_id: None,
    };
    let (permit, wo, status) = (n.permit_code, n.wo_code, n.status);
    let start = n.start_at.unwrap_or("?");
    use crate::permit::ended_under_work::codes;
    let (title, body) = match (n.key, n.end_at) {
        (k, Some(end)) if k == codes::NOTICE_SUSPENDED_AROUND_START => {
            let until = n.until.unwrap_or("?");
            (
                format!(
                    "Travail commencé pendant une suspension du permis {permit} (OT {wo}) / Work started while \
                     permit {permit} was suspended ({wo})"
                ),
                format!(
                    "Reçu par synchronisation : le permis {permit} a été suspendu sur un autre poste du {end} au \
                     {until}, et le travail de l'OT {wo} a commencé ici le {start}, pendant cette suspension. Le \
                     permis est de nouveau en vigueur : examinez le conflit de synchronisation. / Received by sync: \
                     permit {permit} was suspended on another PC from {end} to {until}, and work on {wo} started \
                     here on {start}, during that suspension. The permit is in force again: review the sync \
                     conflict."
                ),
            )
        }
        (k, Some(end)) if k == codes::NOTICE_SUSPENDED_DURING_WORK => {
            let until = n.until.unwrap_or("?");
            (
                format!(
                    "Travail en cours pendant une suspension du permis {permit} (OT {wo}) / Work running while \
                     permit {permit} was suspended ({wo})"
                ),
                format!(
                    "Reçu par synchronisation : le permis {permit} a été suspendu sur un autre poste du {end} au \
                     {until}, alors que le travail de l'OT {wo} était en cours ici (depuis le {start}). Le permis \
                     est de nouveau en vigueur : examinez le conflit de synchronisation. / Received by sync: permit \
                     {permit} was suspended on another PC from {end} to {until}, while work on {wo} was running here \
                     (since {start}). The permit is in force again: review the sync conflict."
                ),
            )
        }
        (k, Some(end)) if k == codes::NOTICE_BEFORE_START => (
            format!(
                "Travail commencé sous un permis déjà terminé : {permit} (OT {wo}) / Work started under an ended \
                 permit: {permit} ({wo})"
            ),
            format!(
                "Reçu par synchronisation : le permis {permit} est passé « {status} » le {end} sur un autre poste, avant \
                 le début du travail de l'OT {wo} ici le {start}. Arrêtez le travail et examinez le conflit de \
                 synchronisation. / Received by sync: permit {permit} became '{status}' on {end} on another PC, \
                 before work on {wo} started here on {start}. Stop the work and review the sync conflict."
            ),
        ),
        (_, Some(end)) => (
            format!(
                "Permis terminé pendant le travail : {permit} (OT {wo}) / Permit ended during the work: {permit} \
                 ({wo})"
            ),
            format!(
                "Reçu par synchronisation : le permis {permit} est passé « {status} » (le {end}) sur un autre poste alors \
                 que le travail de l'OT {wo} est en cours ici depuis le {start}. Arrêtez le travail et examinez le \
                 conflit de synchronisation. / Received by sync: permit {permit} became '{status}' (on {end}) on \
                 another PC while work on {wo} runs here since {start}. Stop the work and review the sync conflict."
            ),
        ),
        // No time of the end (a cancellation): said so, never an empty time.
        (k, None) if k == codes::NOTICE_DURING_WORK_UNTIMED => (
            format!(
                "Permis terminé pendant le travail : {permit} (OT {wo}) / Permit ended during the work: {permit} \
                 ({wo})"
            ),
            format!(
                "Reçu par synchronisation : le permis {permit} est passé « {status} » sur un autre poste (heure non \
                 connue) alors que le travail de l'OT {wo} est en cours ici depuis le {start}. Arrêtez le travail et \
                 examinez le conflit de synchronisation. / Received by sync: permit {permit} became '{status}' on \
                 another PC (time not known) while work on {wo} runs here since {start}. Stop the work and review \
                 the sync conflict."
            ),
        ),
        (_, None) => (
            format!(
                "Permis terminé sur un autre poste, travail commencé ici : {permit} (OT {wo}) / Permit ended on \
                 another PC, work started here: {permit} ({wo})"
            ),
            format!(
                "Reçu par synchronisation : le permis {permit} est passé « {status} » sur un autre poste, à une heure \
                 que la synchronisation ne donne pas ; le travail de l'OT {wo} a commencé ici le {start}. L'ordre ne \
                 peut être établi : vérifiez que le travail est arrêté et examinez le conflit de synchronisation. / \
                 Received by sync: permit {permit} became '{status}' on another PC, at a time sync does not give; \
                 work on {wo} started here on {start}. The order cannot be established: make sure the work is \
                 stopped and review the sync conflict."
            ),
        ),
    };
    send_permit_work_notice(
        conn,
        &route,
        title,
        body,
        NotificationMessage::new(
            n.key,
            json!({
                "permit": permit,
                "work_order": wo,
                "status": status,
                "end_at": n.end_at,
                "reinstated_at": n.until,
                "start_at": n.start_at,
            }),
        ),
    )
    .await;
}
