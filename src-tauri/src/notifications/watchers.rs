//! Condition watchers: notifications derived from the state of synchronised records.
//!
//! Every device runs them from its own copy of the data (sync capture suppressed; seeded
//! identities give the same event and notification ids on every device, so read / acknowledge /
//! archive made on one device apply to the same notification on the others):
//!
//! | watcher                   | condition                                                        |
//! |---------------------------|------------------------------------------------------------------|
//! | `wo_assigned`             | an open work order has a responsible (one event per responsible)  |
//! | `wo_overdue`              | an open work order is past its planned end                       |
//! | `inspection_round_missed` | an inspection round was missed                                   |
//! | `stock_critical`          | an active article's available stock is at or below its reorder point |
//! | `ptw_expiring`            | an open work permit expires within the rule's lead time          |
//! | `ptw_critical`            | an open work permit is past its expiry                           |
//! | `sync_conflict`           | a synchronisation conflict waits for an operator's review        |
//!
//! - **De-duplication**: one open event per record and condition (`dedupe_key`
//!   `watch:<watcher>:<record key>`); the record key holds what makes it a new occurrence (the
//!   responsible, the planned end, the expiry), so re-planning an overdue order raises a new one.
//! - **Resolution**: when a record no longer meets the condition, its event and notifications are
//!   resolved; if it meets it again later, a new event is raised.
//! - **Start**: the first run of a watcher on a device records the conditions already true without
//!   notifying (`notification_watch_state`), so installing or upgrading does not flood the inbox
//!   with old facts. From then on every change is notified.
//! - A watcher whose rule is inactive does not raise anything (its open events still resolve).

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde_json::json;

use crate::errors::AppError;
use crate::notifications::emitter::{self, NotificationEventInput, NotificationMessage};
use crate::notifications::Result;
use crate::sync::identity::seeded_entity_sync_id;

/// The watchers, in run order.
pub const WATCHERS: &[&str] = &[
    "wo_assigned",
    "wo_overdue",
    "inspection_round_missed",
    "stock_critical",
    "ptw_expiring",
    "ptw_critical",
    "sync_conflict",
];

/// One record meeting a watcher's condition.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub record_id: Option<i64>,
    /// Stable on every device (sync ids and the values that make it a new occurrence).
    pub record_key: String,
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

fn s(r: &QueryResult, col: &str) -> Result<String> {
    Ok(r.try_get::<Option<String>>("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("watcher decode '{col}': {e}")))?
        .unwrap_or_default())
}

fn i(r: &QueryResult, col: &str) -> Result<Option<i64>> {
    r.try_get::<Option<i64>>("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("watcher decode '{col}': {e}")))
}

fn f(r: &QueryResult, col: &str) -> Result<f64> {
    Ok(r.try_get::<Option<f64>>("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("watcher decode '{col}': {e}")))?
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
async fn rule_of(conn: &impl ConnectionTrait, category: &str) -> Result<Option<(bool, Option<i64>)>> {
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

const WO_OPEN: &str = "s.macro_state IN ('open', 'executing')";

/// Records meeting the condition of `watcher` now.
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
                ("wo.planned_end IS NOT NULL AND wo.planned_end < ?", vec![ts(now).into()])
            } else {
                ("wo.primary_responsible_id IS NOT NULL", Vec::new())
            };
            let rows = conn
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT wo.id, wo.entity_sync_id AS sid, wo.code, wo.title, wo.planned_end, \
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
                let (key, message_key, fr_title, en_title) = if overdue {
                    (
                        format!("{}|{planned_end}", s(r, "sid")?),
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
                        "wo_assigned",
                        format!("OT affecté — {code}"),
                        format!("Work order assigned — {code}"),
                    )
                };
                out.push(Candidate {
                    record_id: id,
                    record_key: key,
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
                out.push(Candidate {
                    record_id: id,
                    record_key: s(r, "sid")?,
                    source_module: "inspection",
                    message_key: "inspection_round_missed",
                    params: json!({ "code": code, "title": name, "missed_at": s(r, "missed_at")? }),
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
                            COALESCE((SELECT SUM(b.available_qty) FROM stock_balances b WHERE b.article_id = a.id), 0) AS qty \
                     FROM articles a \
                     WHERE a.is_active = 1 AND a.reorder_point > 0 AND a.entity_sync_id IS NOT NULL \
                       AND COALESCE((SELECT SUM(b.available_qty) FROM stock_balances b WHERE b.article_id = a.id), 0) \
                           <= a.reorder_point"
                        .to_string(),
                ))
                .await?;
            for r in &rows {
                let id = i(r, "id")?;
                let code = s(r, "article_code")?;
                let name = s(r, "article_name")?;
                let qty = f(r, "qty")?;
                let reorder = f(r, "reorder_point")?;
                out.push(Candidate {
                    record_id: id,
                    record_key: s(r, "sid")?,
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
        "ptw_expiring" | "ptw_critical" => {
            let expired = watcher == "ptw_critical";
            let (cond, values): (&str, Vec<sea_orm::Value>) = if expired {
                ("wp.expires_at <= ?", vec![ts(now).into()])
            } else {
                let Some(lead) = lead_minutes.filter(|m| *m > 0) else {
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
                         WHERE wp.status IN ('issued', 'active', 'suspended', 'revalidation_required') \
                           AND wp.expires_at IS NOT NULL AND {cond}"
                    ),
                    values,
                ))
                .await?;
            for r in &rows {
                let id = i(r, "id")?;
                let code = s(r, "code")?;
                let expires = s(r, "expires_at")?;
                let type_name = s(r, "type_name")?;
                out.push(Candidate {
                    record_id: id,
                    record_key: format!("{}|{expires}", s(r, "sid")?),
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
                out.push(Candidate {
                    record_id: None,
                    record_key: s(r, "conflict_key")?,
                    source_module: "sync",
                    message_key: "sync_conflict",
                    params: json!({ "entity_type": entity, "conflict_type": kind, "created_at": s(r, "created_at")? }),
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
    pub raised: u64,
    pub recorded_silently: u64,
    pub resolved: u64,
}

/// One round of one watcher. The caller holds a transaction with capture suppressed.
pub async fn run_watcher(
    conn: &impl ConnectionTrait,
    watcher: &str,
    now: DateTime<Utc>,
    fr: bool,
) -> Result<WatchReport> {
    let mut report = WatchReport::default();
    let prefix = format!("watch:{watcher}:");
    let rule = rule_of(conn, watcher).await?;
    let active_rule = rule.is_some_and(|(active, _)| active);
    let lead = rule.and_then(|(_, lead)| lead);
    let current = if active_rule {
        candidates(conn, watcher, now, lead, fr).await?
    } else {
        Vec::new()
    };
    let baselined = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM notification_watch_state WHERE watcher_code = ?",
            [watcher.into()],
        ))
        .await?
        .is_some();

    let open: HashSet<String> = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT dedupe_key FROM notification_events \
             WHERE category_code = ? AND resolved_at IS NULL AND substr(dedupe_key, 1, ?) = ?",
            [
                watcher.into(),
                i64::try_from(prefix.len()).unwrap_or(0).into(),
                prefix.clone().into(),
            ],
        ))
        .await?
        .iter()
        .filter_map(|r| r.try_get::<Option<String>>("", "dedupe_key").ok().flatten())
        .collect();

    let mut seen = HashSet::new();
    for c in &current {
        let dedupe_key = format!("{prefix}{}", c.record_key);
        seen.insert(dedupe_key.clone());
        if open.contains(&dedupe_key) {
            continue;
        }
        let earlier: i64 = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM notification_events WHERE dedupe_key = ?",
                [dedupe_key.clone().into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "n"))
            .transpose()?
            .unwrap_or(0);
        let identity = seeded_entity_sync_id(
            "notification_events",
            &format!("{watcher}|{}|{}", c.record_key, earlier + 1),
        );
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
        emitter::emit(conn, &input, Some(identity), Some(&message), baselined).await?;
        if baselined {
            report.raised += 1;
        } else {
            report.recorded_silently += 1;
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

    if !baselined {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_watch_state (watcher_code, baselined_at) VALUES (?, ?)",
            [watcher.into(), ts(now).into()],
        ))
        .await?;
    }
    Ok(report)
}

/// One round of every watcher; a failing watcher is logged and does not stop the others.
pub async fn run_watchers(conn: &impl ConnectionTrait, now: DateTime<Utc>, fr: bool) -> Result<()> {
    for watcher in WATCHERS {
        match run_watcher(conn, watcher, now, fr).await {
            Ok(r) if r.raised + r.resolved > 0 => {
                tracing::info!(
                    watcher,
                    raised = r.raised,
                    resolved = r.resolved,
                    "notifications::watchers round"
                );
            }
            Ok(_) => {}
            Err(err) => tracing::error!(watcher, error = %err, "notifications::watchers round failed"),
        }
    }
    crate::notifications::conditions::resolve_notifications_of_resolved_events(conn).await?;
    Ok(())
}
