//! The notification inbox of one person: what it lists, what it counts, and bulk "mark all read".
//!
//! **Visibility (checked at read time).** A notification is listed and counted only when the
//! reader may view its module (`notifications::catalog::MODULES`) where its record is: a view
//! permission granted plant-wide shows the module, one granted on org nodes shows the records on
//! those nodes and below (and the records without a node); a category without a known module is
//! never shown. The bell, the inbox views and "mark all read" use the same conditions,
//! so their numbers always agree.
//!
//! **Views.**
//! - `inbox` — not archived, not snoozed, not expired (read, unread and resolved ones).
//! - `unread` — in the inbox, not read, not acknowledged, condition still true.
//! - `to_acknowledge` — in the inbox, the category asks for an acknowledgement, the condition is
//!   still true and nobody notified about the same event acknowledged it yet.
//! - `snoozed`, `archived`, `all`.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::notifications::catalog;

pub const VIEWS: &[&str] = &["inbox", "unread", "to_acknowledge", "snoozed", "archived", "all"];

/// Largest page.
pub const MAX_PAGE: i64 = 200;

const IN_INBOX: &str = "n.archived_at IS NULL AND n.delivery_state NOT IN ('snoozed', 'expired', 'closed')";
const UNREAD: &str = "n.read_at IS NULL AND n.acknowledged_at IS NULL AND n.resolved_at IS NULL";
const REQUIRES_ACK: &str = "COALESCE((SELECT MAX(r2.requires_ack) FROM notification_rules r2 \
     WHERE r2.category_code = e.category_code AND r2.is_active = 1), 0) = 1";
const EVENT_ACKNOWLEDGED: &str = "EXISTS (SELECT 1 FROM notifications n2 \
     WHERE n2.notification_event_id = n.notification_event_id AND n2.acknowledged_at IS NOT NULL)";
const SEVERITY_SQL: &str = "CASE WHEN e.severity IN ('critical', 'error') THEN 'critical' \
     WHEN e.severity = 'warning' THEN 'warning' ELSE 'info' END";

/// What a reader may see, module by module, from the view permission of each module
/// (`catalog::MODULES`) as their roles grant it now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Modules seen plant-wide (a tenant-wide grant, elevated admin, or no permission needed).
    pub everywhere: Vec<String>,
    /// Modules granted on some org nodes only: the notifications of records on those nodes or
    /// below them, and of records that belong to no node.
    pub on_nodes: Vec<(String, Vec<i64>)>,
}

impl Visibility {
    /// Every module plant-wide.
    pub fn all() -> Self {
        Self {
            everywhere: catalog::MODULES.iter().map(|(m, _)| (*m).to_string()).collect(),
            on_nodes: Vec::new(),
        }
    }

    pub fn sees_module(&self, module: &str) -> bool {
        self.everywhere.iter().any(|m| m == module) || self.on_nodes.iter().any(|(m, _)| m == module)
    }

    /// SQL condition on the category `c` and the event `e`.
    fn sql(&self, values: &mut Vec<Value>) -> String {
        let mut parts = Vec::new();
        if !self.everywhere.is_empty() {
            values.extend(self.everywhere.iter().map(|m| m.clone().into()));
            parts.push(format!(
                "c.module_code IN ({})",
                vec!["?"; self.everywhere.len()].join(",")
            ));
        }
        for (module, nodes) in &self.on_nodes {
            values.push(module.clone().into());
            if nodes.is_empty() {
                parts.push("(c.module_code = ? AND e.org_node_id IS NULL)".to_string());
            } else {
                values.extend(nodes.iter().map(|n| (*n).into()));
                parts.push(format!(
                    "(c.module_code = ? AND (e.org_node_id IS NULL OR e.org_node_id IN ({})))",
                    vec!["?"; nodes.len()].join(",")
                ));
            }
        }
        if parts.is_empty() {
            "1 = 0".to_string()
        } else {
            format!("({})", parts.join(" OR "))
        }
    }
}

/// What `user_id` may see now. A role granted plant-wide shows the whole module; a role granted on
/// org nodes (site, area…) shows the notifications of records on those nodes and below, plus those
/// of records without a node. Users with elevated admin access see everything, as in every module.
pub async fn visible_to(db: &sea_orm::DatabaseConnection, user_id: i32) -> AppResult<Visibility> {
    if crate::auth::rbac::user_has_elevated_admin_access(db, user_id).await? {
        return Ok(Visibility::all());
    }
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = Visibility::default();
    for (module, permission) in catalog::MODULES {
        let Some(permission) = permission else {
            out.everywhere.push((*module).to_string());
            continue;
        };
        let grants = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT DISTINCT usa.scope_type, usa.scope_reference
                 FROM user_scope_assignments usa
                 JOIN role_permissions rp ON rp.role_id = usa.role_id
                 JOIN permissions p ON p.id = rp.permission_id
                 WHERE usa.user_id = ? AND p.name = ? AND usa.deleted_at IS NULL
                   AND (usa.valid_from IS NULL OR usa.valid_from <= ?)
                   AND (usa.valid_to IS NULL OR usa.valid_to >= ?)
                   AND (usa.is_emergency = 0 OR usa.emergency_expires_at > ?)",
                [
                    user_id.into(),
                    (*permission).into(),
                    now.clone().into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        if grants.is_empty() {
            continue;
        }
        let mut nodes = Vec::new();
        let mut tenant = false;
        for g in &grants {
            let scope: String = g.try_get("", "scope_type").map_err(|e| decode("scope_type", e))?;
            let reference: Option<String> = g
                .try_get("", "scope_reference")
                .map_err(|e| decode("scope_reference", e))?;
            match scope.as_str() {
                "tenant" => tenant = true,
                "entity" | "org_node" | "site" => {
                    if let Some(id) = reference.and_then(|r| r.trim().parse::<i64>().ok()) {
                        nodes.push(id);
                    }
                }
                // A team-scoped grant still is a grant of the permission: records without a node.
                _ => {}
            }
        }
        if tenant {
            out.everywhere.push((*module).to_string());
        } else {
            out.on_nodes
                .push(((*module).to_string(), node_subtrees(db, &nodes).await?));
        }
    }
    Ok(out)
}

/// The nodes and every node below them.
async fn node_subtrees(db: &impl ConnectionTrait, roots: &[i64]) -> AppResult<Vec<i64>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; roots.len()].join(",");
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "WITH RECURSIVE down(id, lvl) AS (
                    SELECT id, 0 FROM org_nodes WHERE id IN ({marks})
                    UNION SELECT o.id, down.lvl + 1 FROM org_nodes o JOIN down ON o.parent_id = down.id
                    WHERE down.lvl < 32)
                 SELECT DISTINCT id FROM down"
            ),
            roots.iter().map(|r| (*r).into()).collect::<Vec<Value>>(),
        ))
        .await?;
    let mut out: Vec<i64> = rows.iter().filter_map(|r| r.try_get::<i64>("", "id").ok()).collect();
    out.extend_from_slice(roots);
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InboxFilter {
    pub view: Option<String>,
    pub module_code: Option<String>,
    pub severity: Option<String>,
    pub category_code: Option<String>,
    /// Inclusive bounds on the creation time (RFC 3339).
    pub created_from: Option<String>,
    pub created_to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxItem {
    pub id: i64,
    pub title: String,
    pub body: Option<String>,
    pub category_code: String,
    pub module_code: String,
    pub severity: String,
    pub urgency_code: Option<String>,
    pub message_key: Option<String>,
    pub message_params: Option<serde_json::Value>,
    pub delivery_state: String,
    pub created_at: String,
    pub read_at: Option<String>,
    pub acknowledged_at: Option<String>,
    pub snoozed_until: Option<String>,
    pub archived_at: Option<String>,
    pub resolved_at: Option<String>,
    pub action_url: Option<String>,
    pub escalation_level: i64,
    pub requires_ack: bool,
    /// Someone notified about the same event acknowledged it (this person or another).
    pub event_acknowledged_by: Option<String>,
    pub reason_code: Option<String>,
    pub reason_ref: Option<String>,
    /// The condition was already true when this device started watching it: listed as read.
    pub already_true: bool,
    pub source_module: String,
    pub source_record_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleCount {
    pub module_code: String,
    pub unread: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InboxCounts {
    pub unread: i64,
    pub to_acknowledge: i64,
    pub snoozed: i64,
    pub by_module: Vec<ModuleCount>,
}

fn invalid(msg: String) -> AppError {
    AppError::ValidationFailed(vec![format!("GATE_NOTIFICATION_FILTER_INVALID: {msg}")])
}

fn ts(raw: &str, field: &str) -> AppResult<String> {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|d| d.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .map_err(|_| invalid(format!("{field} must be an RFC 3339 time")))
}

fn view_sql(view: &str) -> AppResult<String> {
    Ok(match view {
        "inbox" => IN_INBOX.to_string(),
        "unread" => format!("{IN_INBOX} AND {UNREAD}"),
        "to_acknowledge" => {
            format!("{IN_INBOX} AND n.resolved_at IS NULL AND {REQUIRES_ACK} AND NOT {EVENT_ACKNOWLEDGED}")
        }
        "snoozed" => "n.delivery_state = 'snoozed' AND n.archived_at IS NULL".to_string(),
        "archived" => "n.archived_at IS NOT NULL".to_string(),
        "all" => "1 = 1".to_string(),
        other => return Err(invalid(format!("unknown view '{other}'"))),
    })
}

/// `FROM … WHERE` shared by every inbox query: the reader's notifications they may see.
fn base(user_id: i64, vis: &Visibility, values: &mut Vec<Value>) -> String {
    values.push(user_id.into());
    let visible = vis.sql(values);
    format!(
        "FROM notifications n
         JOIN notification_events e ON e.id = n.notification_event_id
         JOIN notification_categories c ON c.code = e.category_code
         WHERE n.recipient_user_id = ? AND {visible}"
    )
}

fn filter_sql(filter: &InboxFilter, values: &mut Vec<Value>) -> AppResult<String> {
    let mut sql = format!(" AND {}", view_sql(filter.view.as_deref().unwrap_or("inbox"))?);
    if let Some(m) = filter.module_code.as_deref().filter(|m| !m.is_empty()) {
        if catalog::module_view_permission(m).is_none() {
            return Err(invalid(format!("unknown module '{m}'")));
        }
        sql.push_str(" AND c.module_code = ?");
        values.push(m.to_string().into());
    }
    if let Some(s) = filter.severity.as_deref().filter(|s| !s.is_empty()) {
        if !catalog::SEVERITIES.contains(&s) {
            return Err(invalid(format!("unknown severity '{s}'")));
        }
        sql.push_str(&format!(" AND {SEVERITY_SQL} = ?"));
        values.push(s.to_string().into());
    }
    if let Some(cat) = filter.category_code.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND e.category_code = ?");
        values.push(cat.to_string().into());
    }
    if let Some(from) = filter.created_from.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND n.created_at >= ?");
        values.push(ts(from, "created_from")?.into());
    }
    if let Some(to) = filter.created_to.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND n.created_at <= ?");
        values.push(ts(to, "created_to")?.into());
    }
    Ok(sql)
}

fn decode(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("notification inbox decode '{field}': {err}"))
}

/// One page of the reader's notifications, newest first.
pub async fn list(
    conn: &impl ConnectionTrait,
    user_id: i64,
    vis: &Visibility,
    filter: &InboxFilter,
) -> AppResult<Vec<InboxItem>> {
    let mut values = Vec::new();
    let from = base(user_id, vis, &mut values);
    let cond = filter_sql(filter, &mut values)?;
    let limit = filter.limit.unwrap_or(50).clamp(1, MAX_PAGE);
    let offset = filter.offset.unwrap_or(0).max(0);
    values.push(limit.into());
    values.push(offset.into());
    let sql = format!(
        "SELECT n.id, n.title, n.body, e.category_code, c.module_code, {SEVERITY_SQL} AS severity, \
                e.urgency_code, e.message_key, e.message_params_json, n.delivery_state, n.created_at, n.read_at, \
                n.acknowledged_at, n.snoozed_until, n.archived_at, n.resolved_at, n.action_url, n.escalation_level, \
                CASE WHEN {REQUIRES_ACK} THEN 1 ELSE 0 END AS requires_ack, \
                (SELECT ua.display_name FROM notification_acknowledgements a \
                   JOIN notifications n3 ON n3.id = a.notification_id \
                   JOIN user_accounts ua ON ua.id = a.acknowledged_by_id \
                  WHERE n3.notification_event_id = n.notification_event_id ORDER BY a.id LIMIT 1) AS acked_by, \
                CASE WHEN {EVENT_ACKNOWLEDGED} THEN 1 ELSE 0 END AS event_acked, \
                n.reason_code, n.reason_ref, n.already_true, e.source_module, e.source_record_id \
         {from}{cond} ORDER BY n.created_at DESC, n.id DESC LIMIT ? OFFSET ?"
    );
    let rows = conn
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let params: Option<String> = r
            .try_get("", "message_params_json")
            .map_err(|e| decode("message_params_json", e))?;
        let event_acked: i64 = r.try_get("", "event_acked").map_err(|e| decode("event_acked", e))?;
        let acked_by: Option<String> = r.try_get("", "acked_by").map_err(|e| decode("acked_by", e))?;
        out.push(InboxItem {
            id: r.try_get("", "id").map_err(|e| decode("id", e))?,
            title: r.try_get("", "title").map_err(|e| decode("title", e))?,
            body: r.try_get("", "body").map_err(|e| decode("body", e))?,
            category_code: r.try_get("", "category_code").map_err(|e| decode("category_code", e))?,
            module_code: r.try_get("", "module_code").map_err(|e| decode("module_code", e))?,
            severity: r.try_get("", "severity").map_err(|e| decode("severity", e))?,
            urgency_code: r.try_get("", "urgency_code").map_err(|e| decode("urgency_code", e))?,
            message_key: r.try_get("", "message_key").map_err(|e| decode("message_key", e))?,
            message_params: params.and_then(|p| serde_json::from_str(&p).ok()),
            delivery_state: r
                .try_get("", "delivery_state")
                .map_err(|e| decode("delivery_state", e))?,
            created_at: r.try_get("", "created_at").map_err(|e| decode("created_at", e))?,
            read_at: r.try_get("", "read_at").map_err(|e| decode("read_at", e))?,
            acknowledged_at: r
                .try_get("", "acknowledged_at")
                .map_err(|e| decode("acknowledged_at", e))?,
            snoozed_until: r.try_get("", "snoozed_until").map_err(|e| decode("snoozed_until", e))?,
            archived_at: r.try_get("", "archived_at").map_err(|e| decode("archived_at", e))?,
            resolved_at: r.try_get("", "resolved_at").map_err(|e| decode("resolved_at", e))?,
            action_url: r.try_get("", "action_url").map_err(|e| decode("action_url", e))?,
            escalation_level: r
                .try_get("", "escalation_level")
                .map_err(|e| decode("escalation_level", e))?,
            requires_ack: r
                .try_get::<i64>("", "requires_ack")
                .map_err(|e| decode("requires_ack", e))?
                == 1,
            event_acknowledged_by: if event_acked == 1 {
                Some(acked_by.unwrap_or_default())
            } else {
                None
            },
            reason_code: r.try_get("", "reason_code").map_err(|e| decode("reason_code", e))?,
            reason_ref: r.try_get("", "reason_ref").map_err(|e| decode("reason_ref", e))?,
            already_true: r
                .try_get::<i64>("", "already_true")
                .map_err(|e| decode("already_true", e))?
                == 1,
            source_module: r.try_get("", "source_module").map_err(|e| decode("source_module", e))?,
            source_record_id: r
                .try_get("", "source_record_id")
                .map_err(|e| decode("source_record_id", e))?,
        });
    }
    Ok(out)
}

async fn count(conn: &impl ConnectionTrait, user_id: i64, vis: &Visibility, view: &str) -> AppResult<i64> {
    let mut values = Vec::new();
    let from = base(user_id, vis, &mut values);
    let sql = format!("SELECT COUNT(*) AS cnt {from} AND {}", view_sql(view)?);
    Ok(conn
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?
        .map(|r| r.try_get::<i64>("", "cnt"))
        .transpose()
        .map_err(|e| decode("cnt", e))?
        .unwrap_or(0))
}

/// Unread count (the bell) — the same condition as the `unread` view.
pub async fn unread_count(conn: &impl ConnectionTrait, user_id: i64, vis: &Visibility) -> AppResult<i64> {
    count(conn, user_id, vis, "unread").await
}

pub async fn counts(conn: &impl ConnectionTrait, user_id: i64, vis: &Visibility) -> AppResult<InboxCounts> {
    let mut values = Vec::new();
    let from = base(user_id, vis, &mut values);
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT c.module_code AS m, COUNT(*) AS cnt {from} AND {} GROUP BY c.module_code ORDER BY c.module_code",
                view_sql("unread")?
            ),
            values,
        ))
        .await?;
    let mut by_module = Vec::with_capacity(rows.len());
    for r in &rows {
        by_module.push(ModuleCount {
            module_code: r.try_get("", "m").map_err(|e| decode("m", e))?,
            unread: r.try_get("", "cnt").map_err(|e| decode("cnt", e))?,
        });
    }
    Ok(InboxCounts {
        unread: by_module.iter().map(|m| m.unread).sum(),
        to_acknowledge: count(conn, user_id, vis, "to_acknowledge").await?,
        snoozed: count(conn, user_id, vis, "snoozed").await?,
        by_module,
    })
}

/// Marks read every unread notification matching the filter (its view is ignored: only unread
/// ones change). Returns how many.
pub async fn mark_all_read(
    conn: &impl ConnectionTrait,
    user_id: i64,
    vis: &Visibility,
    filter: &InboxFilter,
) -> AppResult<u64> {
    let mut values = Vec::new();
    let from = base(user_id, vis, &mut values);
    let scoped = InboxFilter {
        view: Some("unread".into()),
        limit: None,
        offset: None,
        ..filter.clone()
    };
    let cond = filter_sql(&scoped, &mut values)?;
    let res = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE notifications SET read_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
                 delivery_state = CASE WHEN delivery_state IN ('pending', 'delivered', 'escalated') \
                                       THEN 'read' ELSE delivery_state END \
                 WHERE id IN (SELECT n.id {from}{cond})"
            ),
            values,
        ))
        .await?;
    Ok(res.rows_affected())
}

/// Fails unless the notification belongs to `user_id` and sits in a module the person may view.
pub async fn ensure_visible(
    conn: &impl ConnectionTrait,
    user_id: i64,
    vis: &Visibility,
    notification_id: i64,
) -> AppResult<()> {
    crate::notifications::delivery::ensure_recipient(conn, notification_id, user_id).await?;
    let mut values = Vec::new();
    let from = base(user_id, vis, &mut values);
    values.push(notification_id.into());
    let visible = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT n.id {from} AND n.id = ?"),
            values,
        ))
        .await?
        .is_some();
    if visible {
        Ok(())
    } else {
        Err(AppError::PermissionDenied(
            "This notification is about a module you are not allowed to view.".to_string(),
        ))
    }
}
