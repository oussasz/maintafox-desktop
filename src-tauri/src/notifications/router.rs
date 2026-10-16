//! Who receives a notification.
//!
//! A rule lists audiences (`notifications::catalog`); each audience is resolved against the record
//! the notification is about ([`RecordContext`], loaded from the source module and record id, plus
//! the routing ids an event may carry in its payload) to active user accounts. Every recipient
//! keeps the audience that matched first, so the inbox can say why the person received it.
//!
//! - `assignee`, `requester`, `planner`, `reviewer`, `submitter`, `watcher`: the people the record
//!   names (work order responsible / requester / planner, request reviewer / submitter, inspection
//!   round technician, the work order of a permit or a PM occurrence …).
//! - `team`: members of the record's team, `team_lead`: its leads (crew assignments in force, and
//!   team-scoped role assignments).
//! - `supervisor`: the supervisor (personnel record) of the person concerned, or of the assignee.
//! - `entity_manager`: the responsibles of the record's org node (maintenance owner, approver,
//!   entity manager, and the users whose role is scoped to that node); when nobody holds that
//!   responsibility on the node itself, the nearest parent node that has someone.
//! - `permission:<code>`: active users granted the permission plant-wide or on the record's node.

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::notifications::catalog::Audience;
use crate::notifications::Result;

/// One recipient and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub user_id: i64,
    /// Audience code that matched (`assignee`, `permission`, `escalation` …).
    pub reason: String,
    /// Permission code, escalation level … (display only).
    pub reason_ref: Option<String>,
}

/// The record a notification is about, as far as routing is concerned (user account ids).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordContext {
    pub assignees: Vec<i64>,
    pub requesters: Vec<i64>,
    pub planners: Vec<i64>,
    pub reviewers: Vec<i64>,
    pub submitters: Vec<i64>,
    pub watchers: Vec<i64>,
    pub team_id: Option<i64>,
    pub org_node_id: Option<i64>,
    /// Personnel the record is about (certification holder, blocked person): their supervisor.
    pub person_ids: Vec<i64>,
}

const ACTIVE_USER: &str = "ua.is_active = 1 AND ua.deleted_at IS NULL";
const IN_FORCE_NOW: &str = "(x.valid_from IS NULL OR x.valid_from <= strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
     AND (x.valid_to IS NULL OR x.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))";

fn as_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|v| v.trim().parse().ok()))
}

async fn ids(conn: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> Result<Vec<i64>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    let mut out: Vec<i64> = rows
        .iter()
        .filter_map(|r| r.try_get::<Option<i64>>("", "v").ok().flatten())
        .collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

async fn one(conn: &impl ConnectionTrait, sql: &str, id: i64) -> Result<Option<sea_orm::QueryResult>> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await?)
}

fn opt(row: &sea_orm::QueryResult, col: &str) -> Option<i64> {
    row.try_get::<Option<i64>>("", col).ok().flatten()
}

fn push(v: &mut Vec<i64>, id: Option<i64>) {
    if let Some(id) = id {
        if !v.contains(&id) {
            v.push(id);
        }
    }
}

/// Users of the personnel ids (`user_accounts.personnel_id`).
async fn users_of_personnel(conn: &impl ConnectionTrait, personnel_ids: &[i64]) -> Result<Vec<i64>> {
    if personnel_ids.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; personnel_ids.len()].join(",");
    ids(
        conn,
        &format!("SELECT ua.id AS v FROM user_accounts ua WHERE ua.personnel_id IN ({marks}) AND {ACTIVE_USER}"),
        personnel_ids.iter().map(|i| (*i).into()).collect(),
    )
    .await
}

/// Work-order people of a work order.
async fn work_order_context(conn: &impl ConnectionTrait, wo_id: i64, ctx: &mut RecordContext) -> Result<()> {
    if let Some(r) = one(
        conn,
        "SELECT primary_responsible_id, requester_id, planner_id, assigned_team_id, entity_id \
         FROM work_orders WHERE id = ?",
        wo_id,
    )
    .await?
    {
        push(&mut ctx.assignees, opt(&r, "primary_responsible_id"));
        push(&mut ctx.requesters, opt(&r, "requester_id"));
        push(&mut ctx.planners, opt(&r, "planner_id"));
        ctx.team_id = ctx.team_id.or(opt(&r, "assigned_team_id"));
        ctx.org_node_id = ctx.org_node_id.or(opt(&r, "entity_id"));
    }
    Ok(())
}

/// Loads the routing context of the record a payload points to (`source_module`,
/// `source_record_id`), then applies the routing ids of the payload itself (`team_id` /
/// `assigned_team_id`, `org_node_id` / `entity_id`, `watcher_user_ids`).
pub async fn load_context(conn: &impl ConnectionTrait, payload: &serde_json::Value) -> Result<RecordContext> {
    let mut ctx = RecordContext::default();
    let module = payload.get("source_module").and_then(serde_json::Value::as_str);
    let record = payload.get("source_record_id").and_then(as_i64);
    if let (Some(module), Some(id)) = (module, record) {
        match module {
            "di" | "intervention_requests" => {
                if let Some(r) = one(
                    conn,
                    "SELECT reviewer_id, submitter_id, org_node_id, converted_to_wo_id FROM intervention_requests WHERE id = ?",
                    id,
                )
                .await?
                {
                    push(&mut ctx.reviewers, opt(&r, "reviewer_id"));
                    push(&mut ctx.submitters, opt(&r, "submitter_id"));
                    push(&mut ctx.requesters, opt(&r, "submitter_id"));
                    ctx.org_node_id = opt(&r, "org_node_id");
                    if let Some(wo) = opt(&r, "converted_to_wo_id") {
                        work_order_context(conn, wo, &mut ctx).await?;
                    }
                }
            }
            "work_orders" | "wo" => work_order_context(conn, id, &mut ctx).await?,
            "pm" => {
                if let Some(r) = one(
                    conn,
                    "SELECT po.linked_work_order_id, pp.assigned_team_id, \
                            COALESCE(eo.installed_at_node_id, ep.installed_at_node_id) AS node_id \
                     FROM pm_occurrences po JOIN pm_plans pp ON pp.id = po.pm_plan_id \
                     LEFT JOIN equipment eo ON eo.id = po.equipment_id \
                     LEFT JOIN equipment ep ON pp.asset_scope_type = 'equipment' AND ep.id = pp.asset_scope_id \
                     WHERE po.id = ?",
                    id,
                )
                .await?
                {
                    if let Some(wo) = opt(&r, "linked_work_order_id") {
                        work_order_context(conn, wo, &mut ctx).await?;
                    }
                    ctx.team_id = ctx.team_id.or(opt(&r, "assigned_team_id"));
                    ctx.org_node_id = ctx.org_node_id.or(opt(&r, "node_id"));
                }
            }
            "inspection" => {
                if let Some(r) = one(
                    conn,
                    "SELECT r.assigned_to_id, r.assigned_team_id, t.org_scope_id FROM inspection_rounds r \
                     JOIN inspection_templates t ON t.id = r.template_id WHERE r.id = ?",
                    id,
                )
                .await?
                {
                    if let Some(person) = opt(&r, "assigned_to_id") {
                        ctx.assignees.extend(users_of_personnel(conn, &[person]).await?);
                        ctx.person_ids.push(person);
                    }
                    ctx.team_id = opt(&r, "assigned_team_id");
                    ctx.org_node_id = opt(&r, "org_scope_id");
                }
            }
            "permit" => {
                if let Some(r) = one(
                    conn,
                    "SELECT linked_work_order_id, entity_id FROM work_permits WHERE id = ?",
                    id,
                )
                .await?
                {
                    if let Some(wo) = opt(&r, "linked_work_order_id") {
                        work_order_context(conn, wo, &mut ctx).await?;
                    }
                    ctx.org_node_id = opt(&r, "entity_id").or(ctx.org_node_id);
                }
            }
            "personnel" => {
                if let Some(r) = one(
                    conn,
                    "SELECT primary_entity_id, primary_team_id FROM personnel WHERE id = ?",
                    id,
                )
                .await?
                {
                    ctx.person_ids.push(id);
                    ctx.org_node_id = opt(&r, "primary_entity_id");
                    ctx.team_id = opt(&r, "primary_team_id");
                }
            }
            _ => {}
        }
    }
    if let Some(team) = payload
        .get("team_id")
        .and_then(as_i64)
        .or_else(|| payload.get("assigned_team_id").and_then(as_i64))
    {
        ctx.team_id = Some(team);
    }
    if let Some(node) = payload
        .get("org_node_id")
        .and_then(as_i64)
        .or_else(|| payload.get("entity_id").and_then(as_i64))
    {
        ctx.org_node_id = Some(node);
    }
    if let Some(list) = payload.get("watcher_user_ids").and_then(serde_json::Value::as_array) {
        for w in list {
            push(&mut ctx.watchers, as_i64(w));
        }
    }
    Ok(ctx)
}

/// Members of a team: crew assignments in force (personnel → user) and team-scoped roles.
pub async fn team_member_user_ids(conn: &impl ConnectionTrait, team_id: i64, leads_only: bool) -> Result<Vec<i64>> {
    let lead = if leads_only { "AND x.is_lead = 1" } else { "" };
    let mut out = ids(
        conn,
        &format!(
            "SELECT ua.id AS v FROM personnel_team_assignments x \
             JOIN user_accounts ua ON ua.personnel_id = x.personnel_id \
             WHERE x.team_id = ? {lead} AND {IN_FORCE_NOW} AND {ACTIVE_USER}"
        ),
        vec![team_id.into()],
    )
    .await?;
    if !leads_only {
        out.extend(
            ids(
                conn,
                &format!(
                    "SELECT ua.id AS v FROM user_scope_assignments x JOIN user_accounts ua ON ua.id = x.user_id \
                     WHERE x.scope_type = 'team' AND x.scope_reference = ? AND x.deleted_at IS NULL \
                       AND {IN_FORCE_NOW} AND {ACTIVE_USER}"
                ),
                vec![team_id.to_string().into()],
            )
            .await?,
        );
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Supervisors (personnel records) of the given people and of the given users' personnel.
pub async fn supervisor_user_ids(
    conn: &impl ConnectionTrait,
    person_ids: &[i64],
    user_ids: &[i64],
) -> Result<Vec<i64>> {
    let mut persons: Vec<i64> = person_ids.to_vec();
    if !user_ids.is_empty() {
        let marks = vec!["?"; user_ids.len()].join(",");
        persons.extend(
            ids(
                conn,
                &format!("SELECT personnel_id AS v FROM user_accounts WHERE id IN ({marks})"),
                user_ids.iter().map(|i| (*i).into()).collect(),
            )
            .await?,
        );
    }
    if persons.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; persons.len()].join(",");
    let supervisors = ids(
        conn,
        &format!("SELECT supervisor_id AS v FROM personnel WHERE id IN ({marks})"),
        persons.iter().map(|i| (*i).into()).collect(),
    )
    .await?;
    users_of_personnel(conn, &supervisors).await
}

/// Active users holding a routing responsibility (maintenance owner, approver, entity manager)
/// on an org node, counting only assignments in force now.
async fn node_responsible_user_ids(pool: &impl ConnectionTrait, org_node_id: i64) -> Result<Vec<i64>> {
    ids(
        pool,
        "SELECT ua.id AS v
             FROM org_node_responsibilities r
             JOIN user_accounts ua ON ua.personnel_id = r.person_id
             WHERE r.node_id = ?
               -- Governed codes are seeded uppercase (org.responsibility_type).
               AND UPPER(r.responsibility_type) IN ('ENTITY_MANAGER', 'MAINTENANCE_OWNER', 'APPROVER')
               -- Only assignments in force now: ended ones keep their history row.
               AND (r.valid_from IS NULL OR r.valid_from <= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
               AND (r.valid_to IS NULL OR r.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
               AND ua.is_active = 1",
        vec![org_node_id.into()],
    )
    .await
}

/// Responsibles of the node (responsibilities and node-scoped roles), else of its nearest parent
/// that has some.
pub async fn entity_manager_user_ids(conn: &impl ConnectionTrait, org_node_id: i64) -> Result<Vec<i64>> {
    let nodes = node_chain(conn, org_node_id).await?;
    for node in nodes {
        let mut found = node_responsible_user_ids(conn, node).await?;
        found.extend(
            ids(
                conn,
                &format!(
                    "SELECT ua.id AS v FROM user_scope_assignments x JOIN user_accounts ua ON ua.id = x.user_id \
                     WHERE x.scope_type IN ('entity', 'org_node') AND x.scope_reference = ? \
                       AND x.deleted_at IS NULL AND {IN_FORCE_NOW} AND {ACTIVE_USER}"
                ),
                vec![node.to_string().into()],
            )
            .await?,
        );
        if !found.is_empty() {
            found.sort_unstable();
            found.dedup();
            return Ok(found);
        }
    }
    Ok(Vec::new())
}

/// The node and its parents, nearest first (just the node when it is not in the org tree here).
pub async fn node_chain(conn: &impl ConnectionTrait, org_node_id: i64) -> Result<Vec<i64>> {
    let chain = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE up(id, parent_id, lvl) AS (
                SELECT id, parent_id, 0 FROM org_nodes WHERE id = ?
                UNION ALL SELECT o.id, o.parent_id, up.lvl + 1 FROM org_nodes o JOIN up ON o.id = up.parent_id
                WHERE up.lvl < 32)
             SELECT id FROM up ORDER BY lvl",
            [org_node_id.into()],
        ))
        .await?
        .iter()
        .filter_map(|r| r.try_get::<i64>("", "id").ok())
        .collect::<Vec<_>>();
    Ok(if chain.is_empty() { vec![org_node_id] } else { chain })
}

/// Active users granted `permission` through a role in force — plant-wide, or on the record's node
/// or one of its parents — and the users with elevated admin access, who hold every permission
/// (`auth::rbac::elevated_admin_user_ids`, the rule the permission checks use).
pub async fn permission_holder_user_ids(
    conn: &impl ConnectionTrait,
    permission: &str,
    org_node_id: Option<i64>,
) -> Result<Vec<i64>> {
    let mut values: Vec<sea_orm::Value> = vec![permission.into()];
    let scope = match org_node_id {
        Some(node) => {
            let chain = node_chain(conn, node).await?;
            let marks = vec!["?"; chain.len()].join(",");
            values.extend(chain.iter().map(|n| n.to_string().into()));
            format!("(x.scope_type = 'tenant' OR (x.scope_type IN ('entity', 'org_node', 'site') AND x.scope_reference IN ({marks})))")
        }
        None => "x.scope_type = 'tenant'".to_string(),
    };
    let mut users = ids(
        conn,
        &format!(
            "SELECT ua.id AS v FROM user_scope_assignments x
             JOIN user_accounts ua ON ua.id = x.user_id
             JOIN role_permissions rp ON rp.role_id = x.role_id
             JOIN permissions p ON p.id = rp.permission_id
             WHERE p.name = ? AND {scope} AND x.deleted_at IS NULL AND {IN_FORCE_NOW} AND {ACTIVE_USER}"
        ),
        values,
    )
    .await?;
    users.extend(crate::auth::rbac::elevated_admin_user_ids(conn).await?);
    users.sort_unstable();
    users.dedup();
    Ok(users)
}

/// Active users holding the role named `role_name` (case-insensitive), in force now.
pub async fn role_user_ids(conn: &impl ConnectionTrait, role_name: &str) -> Result<Vec<i64>> {
    ids(
        conn,
        &format!(
            "SELECT ua.id AS v FROM user_scope_assignments x \
             JOIN roles r ON r.id = x.role_id JOIN user_accounts ua ON ua.id = x.user_id \
             WHERE LOWER(TRIM(r.name)) = LOWER(TRIM(?)) AND r.deleted_at IS NULL \
               AND x.deleted_at IS NULL AND {IN_FORCE_NOW} AND {ACTIVE_USER}"
        ),
        vec![role_name.into()],
    )
    .await
}

/// Keeps only active accounts (record columns may name a deactivated user).
async fn active_only(conn: &impl ConnectionTrait, users: &[i64]) -> Result<Vec<i64>> {
    if users.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; users.len()].join(",");
    ids(
        conn,
        &format!("SELECT ua.id AS v FROM user_accounts ua WHERE ua.id IN ({marks}) AND {ACTIVE_USER}"),
        users.iter().map(|i| (*i).into()).collect(),
    )
    .await
}

/// Users of one audience for a record.
pub async fn audience_user_ids(
    conn: &impl ConnectionTrait,
    audience: &Audience,
    ctx: &RecordContext,
) -> Result<Vec<i64>> {
    let users = match audience {
        Audience::Named("assignee") => active_only(conn, &ctx.assignees).await?,
        Audience::Named("requester") => active_only(conn, &ctx.requesters).await?,
        Audience::Named("planner") => active_only(conn, &ctx.planners).await?,
        Audience::Named("submitter") => active_only(conn, &ctx.submitters).await?,
        Audience::Named("watcher") => active_only(conn, &ctx.watchers).await?,
        Audience::Named("reviewer") => active_only(conn, &ctx.reviewers).await?,
        Audience::Named("team") => match ctx.team_id {
            Some(team) => team_member_user_ids(conn, team, false).await?,
            None => Vec::new(),
        },
        Audience::Named("team_lead") => match ctx.team_id {
            Some(team) => team_member_user_ids(conn, team, true).await?,
            None => Vec::new(),
        },
        Audience::Named("supervisor") => supervisor_user_ids(conn, &ctx.person_ids, &ctx.assignees).await?,
        Audience::Named("entity_manager") => match ctx.org_node_id {
            Some(node) => entity_manager_user_ids(conn, node).await?,
            None => Vec::new(),
        },
        Audience::Named(other) => {
            tracing::warn!(audience = other, "notifications::router audience not resolvable");
            Vec::new()
        }
        Audience::Permission(p) => permission_holder_user_ids(conn, p, ctx.org_node_id).await?,
    };
    Ok(users)
}

/// Recipients of a rule's audiences, each with the first audience that matched. A request with no
/// reviewer yet goes to the responsibles of its node (reason `entity_manager`).
pub async fn resolve_audiences(
    conn: &impl ConnectionTrait,
    audiences: &[Audience],
    ctx: &RecordContext,
) -> Result<Vec<Recipient>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for audience in audiences {
        let mut users = audience_user_ids(conn, audience, ctx).await?;
        let mut reason = audience.clone();
        if users.is_empty() && *audience == Audience::Named("reviewer") {
            if let Some(node) = ctx.org_node_id {
                users = entity_manager_user_ids(conn, node).await?;
                reason = Audience::Named("entity_manager");
            }
        }
        for user_id in users {
            if seen.insert(user_id) {
                let (code, reference) = match &reason {
                    Audience::Named(code) => ((*code).to_string(), None),
                    Audience::Permission(p) => ("permission".to_string(), Some(p.clone())),
                };
                out.push(Recipient {
                    user_id,
                    reason: code,
                    reason_ref: reference,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use super::node_responsible_user_ids;

    #[tokio::test]
    async fn node_responsibilities_route_by_governed_code_and_only_while_in_force() {
        let db = crate::db::test_db::migrated().await;
        for sql in [
            "PRAGMA foreign_keys = OFF",
            "INSERT INTO personnel (id, employee_code, full_name, availability_status) VALUES              (1, 'MNT-001', 'Owner', 'available'), (2, 'MNT-002', 'Ended approver', 'available'),              (3, 'MNT-003', 'Planner', 'available'), (4, 'MNT-004', 'Future approver', 'available')",
            "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active,              is_admin, force_password_change, personnel_id, created_at, updated_at) VALUES              (11, 'rt-u1', 'owner', 'Owner', 'local', 1, 0, 0, 1, 'now', 'now'),              (12, 'rt-u2', 'ended', 'Ended', 'local', 1, 0, 0, 2, 'now', 'now'),              (13, 'rt-u3', 'planner', 'Planner', 'local', 1, 0, 0, 3, 'now', 'now'),              (14, 'rt-u4', 'future', 'Future', 'local', 1, 0, 0, 4, 'now', 'now')",
            "INSERT INTO org_node_responsibilities (node_id, responsibility_type, person_id, valid_from, valid_to,              created_at, updated_at) VALUES              (7, 'MAINTENANCE_OWNER', 1, '2026-01-01T00:00:00Z', NULL, 'now', 'now'),              (7, 'APPROVER', 2, '2025-01-01T00:00:00Z', '2025-06-30T00:00:00Z', 'now', 'now'),              (7, 'PLANNER', 3, NULL, NULL, 'now', 'now'),              (7, 'APPROVER', 4, '2999-01-01T00:00:00Z', NULL, 'now', 'now'),              (8, 'MAINTENANCE_OWNER', 3, NULL, NULL, 'now', 'now')",
        ] {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                .await
                .expect(sql);
        }

        let users = node_responsible_user_ids(&db, 7).await.expect("route");
        // Uppercase seeded code matches; the ended, not-yet-started and non-routing ones do not;
        // node 8's owner is not routed for node 7.
        assert_eq!(users, vec![11]);
    }
}
