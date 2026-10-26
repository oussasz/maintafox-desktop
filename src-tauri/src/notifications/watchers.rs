//! Condition watchers: notifications derived from the state of synchronised records.
//!
//! Every device runs them from its own copy of the data (sync capture suppressed). Each occurrence
//! of a condition has an identity built only from values every device shares (sync ids and the
//! moment the condition began), so every device creates the same event and the same notifications
//! and what a recipient does on one device (read, acknowledge, archive) applies on the others:
//!
//! | watcher                   | condition                                              | occurrence (began at)              |
//! |---------------------------|--------------------------------------------------------|------------------------------------|
//! | `wo_assigned`             | an open work order has a responsible                   | order + responsible (last change)  |
//! | `wo_overdue`              | an open work order is past its planned end             | order + planned end                |
//! | `inspection_round_missed` | an inspection round was missed                         | round (window end)                 |
//! | `stock_critical`          | available stock at or below the reorder point          | article + movement that crossed it |
//! | `stock_to_reconcile`      | a location below zero, or counted twice (two PCs)      | article + location (+ counts)      |
//! | `retired_equipment_work`  | an open work order or request on retired equipment     | work order / request               |
//! | `ptw_expiring`            | an open permit expires within the rule's lead time     | permit + expiry (expiry − lead)    |
//! | `ptw_critical`            | an open permit is past its expiry                      | permit + expiry                    |
//! | `sync_conflict`           | a synchronisation conflict waits for review            | conflict (creation)                |
//!
//! - **De-duplication**: one open event per occurrence (`dedupe_key` `watch:<watcher>:<key>`, read
//!   by a range on its index). A resolved occurrence that comes back is reopened, a new occurrence
//!   (re-planned order, new expiry, new crossing) is a new event.
//! - **Resolution**: when the record no longer meets the condition its event and notifications are
//!   resolved.
//! - **Already true when watching started**: each device notes since when it watches each active
//!   rule (`notification_watch_state`; the mark is removed while the rule is inactive, so activating
//!   it later starts a new watch). An occurrence that began before that moment — or whose beginning
//!   is unknown — is still created, with the same ids as everywhere, but as read and marked
//!   `already_true`: it is in the inbox without ringing the bell, and one that must be acknowledged
//!   still waits for its acknowledgement. Nothing is swallowed, nothing floods.

use std::collections::HashSet;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde_json::json;

use crate::errors::AppError;
use crate::notifications::emitter::{self, Arrival, NotificationEventInput, NotificationMessage};
use crate::notifications::Result;
use crate::sync::identity::seeded_entity_sync_id;

/// The watchers, in run order.
pub const WATCHERS: &[&str] = &[
    "wo_assigned",
    "wo_overdue",
    "inspection_round_missed",
    "stock_critical",
    "stock_to_reconcile",
    "retired_equipment_work",
    "ptw_expiring",
    "ptw_critical",
    "sync_conflict",
];

/// One occurrence of a watcher's condition.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub record_id: Option<i64>,
    /// Identical on every device: sync ids and the values that make it a new occurrence.
    pub record_key: String,
    /// When the condition began, when it can be told.
    pub since: Option<DateTime<Utc>>,
    pub source_module: &'static str,
    pub message_key: &'static str,
    pub params: serde_json::Value,
    pub urgency: Option<String>,
    pub action_url: String,
    pub severity: String,
    /// Fallback texts (email, older readers), in the device language.
    pub title: String,
    pub body: String,
}

fn ts(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A stored time in any of the forms the modules write (RFC 3339, local `YYYY-MM-DDTHH:MM[:SS]`
/// read as UTC, or a date).
pub fn parse_time(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return Some(t.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(raw, fmt) {
            return Some(t.and_utc());
        }
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|t| t.and_utc())
}

fn decode(col: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("watcher decode '{col}': {e}"))
}

fn s(r: &QueryResult, col: &str) -> Result<String> {
    Ok(r.try_get::<Option<String>>("", col)
        .map_err(|e| decode(col, e))?
        .unwrap_or_default())
}

fn i(r: &QueryResult, col: &str) -> Result<Option<i64>> {
    r.try_get::<Option<i64>>("", col).map_err(|e| decode(col, e))
}

fn f(r: &QueryResult, col: &str) -> Result<f64> {
    Ok(r.try_get::<Option<f64>>("", col)
        .map_err(|e| decode(col, e))?
        .unwrap_or(0.0))
}

fn text(fr: bool, fr_text: String, en_text: String) -> String {
    if fr {
        fr_text
    } else {
        en_text
    }
}

/// Category default severity (SYSTEM catalog).
async fn category_severity(conn: &impl ConnectionTrait, category: &str) -> Result<String> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT default_severity FROM notification_categories WHERE code = ?",
            [category.into()],
        ))
        .await?
        .map(|r| r.try_get::<String>("", "default_severity"))
        .transpose()?
        .unwrap_or_else(|| "info".to_string()))
}

/// `(is_active, lead_time_minutes)` of the watcher's rule.
pub(crate) async fn rule_of(conn: &impl ConnectionTrait, category: &str) -> Result<Option<(bool, Option<i64>)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active, lead_time_minutes FROM notification_rules WHERE category_code = ? LIMIT 1",
            [category.into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => Some((r.try_get::<i64>("", "is_active")? == 1, i(&r, "lead_time_minutes")?)),
        None => None,
    })
}

/// OTs whose work is still to do (`wo::backlog::OPEN_STATUS_SQL`, "Open (work to do)").
const WO_OPEN: &str = concat!("s.code IN ", crate::wo_open_statuses_sql!());

/// The movement that brought the available stock of an article from above its reorder point to
/// at or below it most recently, with when it happened (`None`: always below in the ledger).
/// Movements are read newest first in the order every device shares (time, then sync id), the
/// available stock is walked back from today's level (`inventory::ledger` effects).
async fn reorder_crossing(
    conn: &impl ConnectionTrait,
    article_id: i64,
    available_now: f64,
    reorder_point: f64,
) -> Result<Option<(String, Option<DateTime<Utc>>)>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id, movement_type, quantity, performed_at FROM inventory_transactions \
             WHERE article_id = ? ORDER BY performed_at DESC, entity_sync_id DESC",
            [article_id.into()],
        ))
        .await?;
    let mut available = available_now;
    for r in &rows {
        let movement: String = r.try_get("", "movement_type").map_err(|e| decode("movement_type", e))?;
        let quantity = f(r, "quantity")?;
        let Some((on_hand, reserved)) = crate::inventory::ledger::movement_balance_effect(&movement, quantity) else {
            continue;
        };
        let before = available - (on_hand - reserved);
        if before > reorder_point && available <= reorder_point {
            let sid = s(r, "entity_sync_id")?;
            return Ok(Some((sid, parse_time(&s(r, "performed_at")?))));
        }
        available = before;
    }
    Ok(None)
}

/// Occurrences of the condition of `watcher` now.
pub async fn candidates(
    conn: &impl ConnectionTrait,
    watcher: &str,
    now: DateTime<Utc>,
    lead_minutes: Option<i64>,
    fr: bool,
) -> Result<Vec<Candidate>> {
    let severity = category_severity(conn, watcher).await?;
    let mut out = Vec::new();
    match watcher {
        "wo_assigned" | "wo_overdue" => {
            let overdue = watcher == "wo_overdue";
            let (cond, values): (&str, Vec<sea_orm::Value>) = if overdue {
                (
                    "wo.planned_end IS NOT NULL AND wo.planned_end < ?",
                    vec![ts(now).into()],
                )
            } else {
                ("wo.primary_responsible_id IS NOT NULL", Vec::new())
            };
            let rows = conn
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT wo.id, wo.entity_sync_id AS sid, wo.code, wo.title, wo.planned_end, wo.updated_at, \
                                ua.entity_sync_id AS assignee_sid, ua.display_name AS assignee, ul.code AS urgency \
                         FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                         LEFT JOIN user_accounts ua ON ua.id = wo.primary_responsible_id \
                         LEFT JOIN reference_values ul ON ul.id = wo.urgency_id \
                         WHERE {WO_OPEN} AND wo.entity_sync_id IS NOT NULL AND {cond}"
                    ),
                    values,
                ))
                .await?;
            for r in &rows {
                let id = i(r, "id")?;
                let code = s(r, "code")?;
                let title = s(r, "title")?;
                let planned_end = s(r, "planned_end")?;
                let (key, since, message_key, fr_title, en_title) = if overdue {
                    (
                        format!("{}|{planned_end}", s(r, "sid")?),
                        parse_time(&planned_end),
                        "wo_overdue",
                        format!("OT en retard — {code}"),
                        format!("Work order overdue — {code}"),
                    )
                } else {
                    let assignee_sid = s(r, "assignee_sid")?;
                    if assignee_sid.is_empty() {
                        continue;
                    }
                    (
                        format!("{}|{assignee_sid}", s(r, "sid")?),
                        // No assignment time is kept: the order's last change bounds it.
                        parse_time(&s(r, "updated_at")?),
                        "wo_assigned",
                        format!("OT affecté — {code}"),
                        format!("Work order assigned — {code}"),
                    )
                };
                out.push(Candidate {
                    record_id: id,
                    record_key: key,
                    since,
                    source_module: "work_orders",
                    message_key,
                    params: json!({ "code": code, "title": title, "planned_end": planned_end, "assignee": s(r, "assignee")? }),
                    urgency: r.try_get::<Option<String>>("", "urgency").ok().flatten(),
                    action_url: format!("/work-orders?openWo={}", id.unwrap_or_default()),
                    severity: severity.clone(),
                    title: text(fr, fr_title, en_title),
                    body: title,
                });
            }
        }
        "inspection_round_missed" => {
            let rows = conn
                .query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT r.id, r.entity_sync_id AS sid, t.code, t.name, COALESCE(r.missed_at, r.window_end_at) AS missed_at \
                     FROM inspection_rounds r JOIN inspection_templates t ON t.id = r.template_id \
                     WHERE r.status = 'missed'"
                        .to_string(),
                ))
                .await?;
            for r in &rows {
                let id = i(r, "id")?;
                let code = s(r, "code")?;
                let name = s(r, "name")?;
                let missed_at = s(r, "missed_at")?;
                out.push(Candidate {
                    record_id: id,
                    record_key: s(r, "sid")?,
                    since: parse_time(&missed_at),
                    source_module: "inspection",
                    message_key: "inspection_round_missed",
                    params: json!({ "code": code, "title": name, "missed_at": missed_at }),
                    urgency: None,
                    action_url: format!("/inspections?openRound={}", id.unwrap_or_default()),
                    severity: severity.clone(),
                    title: text(
                        fr,
                        format!("Ronde manquée — {code}"),
                        format!("Inspection round missed — {code}"),
                    ),
                    body: name,
                });
            }
        }
        "stock_critical" => {
            let rows = conn
                .query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT a.id, a.entity_sync_id AS sid, a.article_code, a.article_name, a.reorder_point, \
                            COALESCE(b.qty, 0) AS qty \
                     FROM articles a \
                     LEFT JOIN (SELECT article_id, SUM(available_qty) AS qty FROM stock_balances GROUP BY article_id) b \
                            ON b.article_id = a.id \
                     WHERE a.is_active = 1 AND a.reorder_point > 0 AND a.entity_sync_id IS NOT NULL \
                       AND COALESCE(b.qty, 0) <= a.reorder_point"
                        .to_string(),
                ))
                .await?;
            for r in &rows {
                let id = i(r, "id")?;
                let code = s(r, "article_code")?;
                let name = s(r, "article_name")?;
                let qty = f(r, "qty")?;
                let reorder = f(r, "reorder_point")?;
                let crossing = match id {
                    Some(article) => reorder_crossing(conn, article, qty, reorder).await?,
                    None => None,
                };
                let (episode, since) = match crossing {
                    Some((movement, at)) => (movement, at),
                    None => ("below-from-start".to_string(), None),
                };
                out.push(Candidate {
                    record_id: id,
                    record_key: format!("{}|{episode}", s(r, "sid")?),
                    since,
                    source_module: "inventory",
                    message_key: "stock_critical",
                    params: json!({ "code": code, "title": name, "available": qty, "reorder_point": reorder }),
                    urgency: None,
                    action_url: format!("/inventory?articleId={}", id.unwrap_or_default()),
                    severity: severity.clone(),
                    title: text(
                        fr,
                        format!("Stock au seuil de réapprovisionnement — {code}"),
                        format!("Stock at reorder point — {code}"),
                    ),
                    body: text(
                        fr,
                        format!("{name} : {qty} disponible(s), seuil {reorder}."),
                        format!("{name}: {qty} available, reorder point {reorder}."),
                    ),
                });
            }
        }
        // Stock to reconcile after sync (`inventory::negative_stock`): one notice per article and
        // location below zero and per double count (named by the counts it is made of). The
        // negative occurrence is the pair itself: no value read from the ledger can name the
        // opening the same way on every PC and stay fixed while a movement dated earlier arrives
        // late, so a later item of the same pair reopens the same notice instead of a new one. The
        // movement that took it below zero (ledger order every PC shares) only dates it.
        "stock_to_reconcile" => {
            use crate::inventory::negative_stock::{double_count_pairs, negative_crossing, QTY_TOLERANCE};
            const COLUMNS: &str = "SELECT a.id AS article_id, a.entity_sync_id AS asid, a.article_code, a.article_name,                     sl.id AS location_id, sl.entity_sync_id AS lsid, sl.code AS location_code, w.code AS warehouse_code,                     COALESCE(sb.on_hand_qty, 0) AS on_hand_qty";
            const SYNCED: &str = "a.entity_sync_id IS NOT NULL AND sl.entity_sync_id IS NOT NULL";
            let action_url = crate::inventory::negative_stock::LIST_URL.to_string();
            let negatives = conn
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "{COLUMNS} FROM stock_balances sb JOIN articles a ON a.id = sb.article_id                          JOIN stock_locations sl ON sl.id = sb.location_id JOIN warehouses w ON w.id = sl.warehouse_id                          WHERE {SYNCED} AND sb.on_hand_qty < ? ORDER BY a.id, sl.id"
                    ),
                    [(-QTY_TOLERANCE).into()],
                ))
                .await?;
            for r in &negatives {
                let (article_id, location_id) = (
                    i(r, "article_id")?.unwrap_or_default(),
                    i(r, "location_id")?.unwrap_or_default(),
                );
                let on_hand = f(r, "on_hand_qty")?;
                // When the current shortage opened: the crossing, or the first movement when the
                // ledger shows the location below zero from the start (CMMS Advisor: the notice
                // says it; a later shortage reopens the notice with its own date).
                let opened_raw = match negative_crossing(conn, article_id, location_id, on_hand).await? {
                    Some((_, at)) => at,
                    None => conn
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT MIN(performed_at) AS first_at FROM inventory_transactions \
                             WHERE article_id = ? AND location_id = ?",
                            [article_id.into(), location_id.into()],
                        ))
                        .await?
                        .and_then(|row| row.try_get::<Option<String>>("", "first_at").ok().flatten()),
                };
                let since = opened_raw.as_deref().and_then(parse_time);
                let opened_at = since.map(ts).unwrap_or_default();
                // The stored fallback text (email, older readers) reads the date; the in-app text is
                // localised from `opened_at` (`notifications:messages.stock_negative`).
                let since_text = |fr: bool| match since {
                    Some(at) if fr => format!(", sous zéro depuis le {} UTC", at.format("%d/%m/%Y %H:%M")),
                    Some(at) => format!(", below zero since {} UTC", at.format("%Y-%m-%d %H:%M")),
                    None => String::new(),
                };
                let code = s(r, "article_code")?;
                let name = s(r, "article_name")?;
                let location = format!("{}/{}", s(r, "warehouse_code")?, s(r, "location_code")?);
                out.push(Candidate {
                    record_id: Some(article_id),
                    record_key: format!("negative|{}|{}", s(r, "asid")?, s(r, "lsid")?),
                    since,
                    source_module: "inventory",
                    message_key: "stock_negative",
                    params: json!({
                        "code": code,
                        "title": name,
                        "location": location,
                        "on_hand": on_hand,
                        "opened_at": opened_at,
                    }),
                    urgency: None,
                    action_url: action_url.clone(),
                    severity: severity.clone(),
                    title: text(
                        fr,
                        format!("Stock négatif à rapprocher — {code}"),
                        format!("Negative stock to reconcile — {code}"),
                    ),
                    body: text(
                        fr,
                        format!("{name} à {location} : {on_hand} en stock{}.", since_text(true)),
                        format!("{name} at {location}: {on_hand} on hand{}.", since_text(false)),
                    ),
                });
            }
            for (article_id, location_id, counts) in double_count_pairs(conn).await? {
                let Some(r) = conn
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        format!(
                            "{COLUMNS} FROM articles a JOIN stock_locations sl ON sl.id = ?                              JOIN warehouses w ON w.id = sl.warehouse_id                              LEFT JOIN stock_balances sb ON sb.article_id = a.id AND sb.location_id = sl.id                              WHERE {SYNCED} AND a.id = ?"
                        ),
                        [location_id.into(), article_id.into()],
                    ))
                    .await?
                else {
                    continue;
                };
                let code = s(&r, "article_code")?;
                let name = s(&r, "article_name")?;
                let location = format!("{}/{}", s(&r, "warehouse_code")?, s(&r, "location_code")?);
                let ids: Vec<&str> = counts.iter().map(|c| c.count_sync_id.as_str()).collect();
                let since = counts.iter().filter_map(|c| parse_time(&c.counted_at)).max();
                out.push(Candidate {
                    record_id: Some(article_id),
                    record_key: format!("double|{}|{}|{}", s(&r, "asid")?, s(&r, "lsid")?, ids.join(",")),
                    since,
                    source_module: "inventory",
                    message_key: "stock_double_count",
                    params: json!({ "code": code, "title": name, "location": location, "count": counts.len() }),
                    urgency: None,
                    action_url: action_url.clone(),
                    severity: severity.clone(),
                    title: text(
                        fr,
                        format!("Double comptage à vérifier — {code}"),
                        format!("Double count to verify — {code}"),
                    ),
                    body: text(
                        fr,
                        format!(
                            "{name} à {location} : {} comptages sur des postes différents.",
                            counts.len()
                        ),
                        format!("{name} at {location}: {} counts on different PCs.", counts.len()),
                    ),
                });
            }
        }
        "ptw_expiring" | "ptw_critical" => {
            let expired = watcher == "ptw_critical";
            let lead = lead_minutes.filter(|m| *m > 0);
            let (cond, values): (&str, Vec<sea_orm::Value>) = if expired {
                ("wp.expires_at <= ?", vec![ts(now).into()])
            } else {
                let Some(lead) = lead else {
                    // No lead time configured for this rule: nothing is "expiring soon".
                    return Ok(out);
                };
                (
                    "wp.expires_at > ? AND wp.expires_at <= ?",
                    vec![ts(now).into(), ts(now + Duration::minutes(lead)).into()],
                )
            };
            let rows = conn
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT wp.id, wp.entity_sync_id AS sid, wp.code, wp.expires_at, pt.name AS type_name, \
                                e.asset_id_code \
                         FROM work_permits wp LEFT JOIN permit_types pt ON pt.id = wp.permit_type_id \
                         LEFT JOIN equipment e ON e.id = wp.asset_id \
                         WHERE wp.status IN {open} \
                           AND wp.expires_at IS NOT NULL AND {cond}",
                        open = crate::permit::queries::OPEN_PERMIT_STATUSES_SQL
                    ),
                    values,
                ))
                .await?;
            for r in &rows {
                let id = i(r, "id")?;
                let code = s(r, "code")?;
                let expires = s(r, "expires_at")?;
                let type_name = s(r, "type_name")?;
                let expiry = parse_time(&expires);
                let since = if expired {
                    expiry
                } else {
                    expiry.zip(lead).map(|(e, l)| e - Duration::minutes(l))
                };
                out.push(Candidate {
                    record_id: id,
                    record_key: format!("{}|{expires}", s(r, "sid")?),
                    since,
                    source_module: "permit",
                    message_key: if expired { "ptw_expired" } else { "ptw_expiring" },
                    params: json!({ "code": code, "title": type_name, "asset": s(r, "asset_id_code")?, "expires_at": expires }),
                    urgency: None,
                    action_url: format!("/permits?permit={}", id.unwrap_or_default()),
                    severity: severity.clone(),
                    title: if expired {
                        text(fr, format!("Permis expiré encore ouvert — {code}"), format!("Expired work permit still open — {code}"))
                    } else {
                        text(fr, format!("Permis bientôt expiré — {code}"), format!("Work permit expiring — {code}"))
                    },
                    body: type_name,
                });
            }
        }
        "sync_conflict" => {
            let rows = conn
                .query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT conflict_key, entity_type, conflict_type, created_at FROM sync_conflicts \
                     WHERE requires_operator_review = 1 AND status IN ('new', 'triaged', 'escalated')"
                        .to_string(),
                ))
                .await?;
            for r in &rows {
                let entity = s(r, "entity_type")?;
                let kind = s(r, "conflict_type")?;
                let created = s(r, "created_at")?;
                out.push(Candidate {
                    record_id: None,
                    record_key: s(r, "conflict_key")?,
                    since: parse_time(&created),
                    source_module: "sync",
                    message_key: "sync_conflict",
                    params: json!({ "entity_type": entity, "conflict_type": kind, "created_at": created }),
                    urgency: None,
                    action_url: "/sync-center".to_string(),
                    severity: severity.clone(),
                    title: text(
                        fr,
                        "Conflit de synchronisation à examiner".into(),
                        "Synchronisation conflict to review".into(),
                    ),
                    body: format!("{entity} · {kind}"),
                });
            }
        }
        // Work on retired equipment, crossed in sync (`wo::retired_equipment_review`, AU-73b): one
        // notice per open work order or request, its sync id the occurrence (the same on every PC).
        "retired_equipment_work" => {
            out.extend(crate::wo::retired_equipment_review::notice_candidates(conn, &severity, fr).await?);
        }
        other => {
            return Err(AppError::Internal(anyhow::anyhow!(
                "unknown notification watcher '{other}'"
            )));
        }
    }
    Ok(out)
}

/// Counts of one round of a watcher.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchReport {
    /// New occurrences notified.
    pub raised: u64,
    /// Occurrences already true when this device started watching: listed as read.
    pub already_true: u64,
    pub resolved: u64,
}

/// `[from, to)` of the dedupe keys of a watcher (`watch:<code>:` … `watch:<code>;`), a range on
/// the dedupe-key index.
fn key_range(watcher: &str) -> (String, String) {
    (format!("watch:{watcher}:"), format!("watch:{watcher};"))
}

/// Since when this device watches the rule of `watcher`: set when the rule is first seen active,
/// removed while it is inactive.
async fn watching_since(
    conn: &impl ConnectionTrait,
    watcher: &str,
    active: bool,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    if !active {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM notification_watch_state WHERE watcher_code = ?",
            [watcher.into()],
        ))
        .await?;
        return Ok(None);
    }
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO notification_watch_state (watcher_code, watching_since) VALUES (?, ?)",
        [watcher.into(), ts(now).into()],
    ))
    .await?;
    let raw: String = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT watching_since FROM notification_watch_state WHERE watcher_code = ?",
            [watcher.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("watch state of {watcher} missing")))?
        .try_get("", "watching_since")?;
    Ok(Some(parse_time(&raw).unwrap_or(now)))
}

/// One round of one watcher. The caller holds a transaction with capture suppressed.
pub async fn run_watcher(
    conn: &impl ConnectionTrait,
    watcher: &str,
    now: DateTime<Utc>,
    fr: bool,
) -> Result<WatchReport> {
    let mut report = WatchReport::default();
    let (from, to) = key_range(watcher);
    let rule = rule_of(conn, watcher).await?;
    let active = rule.is_some_and(|(active, _)| active);
    let lead = rule.and_then(|(_, lead)| lead);
    let since_watch = watching_since(conn, watcher, active, now).await?;
    let current = match since_watch {
        Some(_) => candidates(conn, watcher, now, lead, fr).await?,
        None => Vec::new(),
    };

    let open: HashSet<String> = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT dedupe_key FROM notification_events \
             WHERE dedupe_key >= ? AND dedupe_key < ? AND resolved_at IS NULL",
            [from.clone().into(), to.into()],
        ))
        .await?
        .iter()
        .filter_map(|r| r.try_get::<Option<String>>("", "dedupe_key").ok().flatten())
        .collect();

    let mut seen = HashSet::new();
    for c in &current {
        let dedupe_key = format!("{from}{}", c.record_key);
        seen.insert(dedupe_key.clone());
        if open.contains(&dedupe_key) {
            continue;
        }
        let already_true = match (c.since, since_watch) {
            (Some(began), Some(watching)) => began < watching,
            _ => true,
        };
        let identity = seeded_entity_sync_id("notification_events", &format!("{watcher}|{}", c.record_key));
        let input = NotificationEventInput {
            source_module: c.source_module.to_string(),
            source_record_id: c.record_id.map(|id| id.to_string()),
            event_code: watcher.to_string(),
            category_code: watcher.to_string(),
            severity: c.severity.clone(),
            dedupe_key: Some(dedupe_key),
            payload_json: None,
            title: c.title.clone(),
            body: Some(c.body.clone()),
            action_url: Some(c.action_url.clone()),
        };
        let message = NotificationMessage::new(c.message_key, c.params.clone()).with_urgency(c.urgency.clone());
        let arrival = if already_true {
            Arrival::AlreadyTrue
        } else {
            Arrival::New
        };
        emitter::emit(conn, &input, Some(identity), Some(&message), arrival).await?;
        if already_true {
            report.already_true += 1;
        } else {
            report.raised += 1;
        }
    }

    for key in open.difference(&seen) {
        let res = conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE notification_events SET resolved_at = ? WHERE dedupe_key = ? AND resolved_at IS NULL",
                [ts(now).into(), key.clone().into()],
            ))
            .await?;
        report.resolved += res.rows_affected();
    }
    crate::notifications::conditions::resolve_notifications_of_resolved_events(conn).await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::parse_time;

    #[test]
    fn stored_times_of_every_form_are_read() {
        assert!(parse_time("2026-10-15T10:00:00Z").is_some());
        assert!(parse_time("2026-10-15T10:00:00+01:00").is_some());
        assert_eq!(parse_time("2026-10-15T10:00"), parse_time("2026-10-15T10:00:00Z"));
        assert!(parse_time("2026-10-15").is_some());
        assert!(parse_time("").is_none());
        assert!(parse_time("soon").is_none());
    }
}
