use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::notifications::{Result, SqlitePool};

#[derive(Debug, Clone, Default)]
pub struct RoutingResult {
    pub recipient_user_ids: Vec<i64>,
    pub recipient_role_ids: Vec<i64>,
}

pub async fn resolve_recipients(
    pool: &SqlitePool,
    routing_mode: &str,
    payload: &serde_json::Value,
) -> Result<RoutingResult> {
    let mut user_ids = BTreeSet::new();
    let mut role_ids = BTreeSet::new();

    match routing_mode {
        "assignee" => {
            if let Some((source_module, source_id)) = extract_source(payload) {
                if source_module == "work_orders" || source_module == "wo" {
                    if let Some(user_id) = query_optional_i64(
                        pool,
                        "SELECT primary_responsible_id FROM work_orders WHERE id = ?",
                        [source_id.into()],
                        "primary_responsible_id",
                    )
                    .await?
                    {
                        user_ids.insert(user_id);
                    }
                } else if source_module == "intervention_requests" || source_module == "di" {
                    if let Some(user_id) = query_optional_i64(
                        pool,
                        "SELECT reviewer_id FROM intervention_requests WHERE id = ?",
                        [source_id.into()],
                        "reviewer_id",
                    )
                    .await?
                    {
                        user_ids.insert(user_id);
                    }
                } else if source_module == "pm" {
                    if let Some(user_id) = query_optional_i64(
                        pool,
                        "SELECT wo.primary_responsible_id
                         FROM pm_occurrences po
                         JOIN work_orders wo ON wo.id = po.linked_work_order_id
                         WHERE po.id = ?",
                        [source_id.into()],
                        "primary_responsible_id",
                    )
                    .await?
                    {
                        user_ids.insert(user_id);
                    } else if let Some(team_id) = query_optional_i64(
                        pool,
                        "SELECT pp.assigned_group_id
                         FROM pm_occurrences po
                         JOIN pm_plans pp ON pp.id = po.pm_plan_id
                         WHERE po.id = ?",
                        [source_id.into()],
                        "assigned_group_id",
                    )
                    .await?
                    {
                        let rows = pool
                            .query_all(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                "SELECT DISTINCT usa.user_id
                                 FROM user_scope_assignments usa
                                 JOIN user_accounts ua ON ua.id = usa.user_id
                                 WHERE usa.scope_type = 'team'
                                   AND usa.scope_reference = ?
                                   AND usa.deleted_at IS NULL
                                   AND ua.is_active = 1
                                   AND (usa.valid_to IS NULL OR usa.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                                [team_id.to_string().into()],
                            ))
                            .await?;
                        for row in rows {
                            if let Ok(user_id) = row.try_get::<i64>("", "user_id") {
                                user_ids.insert(user_id);
                            }
                        }
                    }
                }
            }
        }
        "reviewer" => {
            if let Some((source_module, source_id)) = extract_source(payload) {
                if source_module == "intervention_requests" || source_module == "di" {
                    if let Some(user_id) = query_optional_i64(
                        pool,
                        "SELECT reviewer_id FROM intervention_requests WHERE id = ?",
                        [source_id.into()],
                        "reviewer_id",
                    )
                    .await?
                    {
                        user_ids.insert(user_id);
                    } else if let Some(org_node_id) = query_optional_i64(
                        pool,
                        "SELECT org_node_id FROM intervention_requests WHERE id = ?",
                        [source_id.into()],
                        "org_node_id",
                    )
                    .await?
                    {
                        let rows = pool
                            .query_all(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                "SELECT ua.id AS user_id
                                 FROM org_node_responsibilities r
                                 JOIN user_accounts ua ON ua.personnel_id = r.person_id
                                 WHERE r.node_id = ?
                                   AND r.responsibility_type IN ('entity_manager', 'maintenance_owner', 'approver')
                                   AND ua.is_active = 1",
                                [org_node_id.into()],
                            ))
                            .await?;
                        for row in rows {
                            if let Ok(user_id) = row.try_get::<i64>("", "user_id") {
                                user_ids.insert(user_id);
                            }
                        }
                    }
                } else if source_module == "work_orders" || source_module == "wo" {
                    if let Some(user_id) = query_optional_i64(
                        pool,
                        "SELECT planner_id FROM work_orders WHERE id = ?",
                        [source_id.into()],
                        "planner_id",
                    )
                    .await?
                    {
                        user_ids.insert(user_id);
                    }
                }
            }
        }
        "role" => {
            if let Some(role_name) = payload
                .get("routing_role_name")
                .and_then(serde_json::Value::as_str)
            {
                if let Some(role_id) = query_optional_i64(
                    pool,
                    "SELECT id FROM roles WHERE name = ? LIMIT 1",
                    [role_name.into()],
                    "id",
                )
                .await?
                {
                    role_ids.insert(role_id);
                }
            }
        }
        "team" => {
            let team_id = payload
                .get("team_id")
                .and_then(as_i64)
                .or_else(|| payload.get("assigned_group_id").and_then(as_i64));
            if let Some(team_id) = team_id {
                let rows = pool
                    .query_all(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT DISTINCT user_id
                         FROM user_scope_assignments
                         WHERE scope_type = 'team'
                           AND scope_reference = ?
                           AND deleted_at IS NULL
                           AND (valid_to IS NULL OR valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                        [team_id.to_string().into()],
                    ))
                    .await?;
                for row in rows {
                    if let Ok(user_id) = row.try_get::<i64>("", "user_id") {
                        user_ids.insert(user_id);
                    }
                }
            }
        }
        "entity_manager" => {
            let entity_ref = payload
                .get("entity_id")
                .and_then(as_i64)
                .or_else(|| payload.get("org_node_id").and_then(as_i64));
            if let Some(entity_ref) = entity_ref {
                let rows = pool
                    .query_all(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT DISTINCT usa.user_id
                         FROM user_scope_assignments usa
                         JOIN user_accounts ua ON ua.id = usa.user_id
                         WHERE usa.scope_type IN ('entity', 'org_node')
                           AND usa.scope_reference = ?
                           AND usa.deleted_at IS NULL
                           AND ua.is_active = 1
                           AND (usa.valid_to IS NULL OR usa.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                        [entity_ref.to_string().into()],
                    ))
                    .await?;
                for row in rows {
                    if let Ok(user_id) = row.try_get::<i64>("", "user_id") {
                        user_ids.insert(user_id);
                    }
                }
            } else if let Some((source_module, source_id)) = extract_source(payload) {
                if source_module == "pm" {
                    if let Some(node_id) = query_optional_i64(
                        pool,
                        "SELECT e.installed_at_node_id
                         FROM pm_occurrences po
                         JOIN pm_plans pp ON pp.id = po.pm_plan_id
                         JOIN equipment e ON e.id = pp.asset_scope_id
                         WHERE po.id = ? AND pp.asset_scope_type = 'equipment'",
                        [source_id.into()],
                        "installed_at_node_id",
                    )
                    .await?
                    {
                        let rows = pool
                            .query_all(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                "SELECT DISTINCT usa.user_id
                                 FROM user_scope_assignments usa
                                 JOIN user_accounts ua ON ua.id = usa.user_id
                                 WHERE usa.scope_type IN ('entity', 'org_node')
                                   AND usa.scope_reference = ?
                                   AND usa.deleted_at IS NULL
                                   AND ua.is_active = 1
                                   AND (usa.valid_to IS NULL OR usa.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                                [node_id.to_string().into()],
                            ))
                            .await?;
                        for row in rows {
                            if let Ok(user_id) = row.try_get::<i64>("", "user_id") {
                                user_ids.insert(user_id);
                            }
                        }
                    }
                }
            }
        }
        "watcher" => {
            if let Some(watchers) = payload
                .get("watcher_user_ids")
                .and_then(serde_json::Value::as_array)
            {
                for watcher in watchers {
                    if let Some(user_id) = as_i64(watcher) {
                        user_ids.insert(user_id);
                    }
                }
            }
        }
        "manual" => {}
        _ => {
            tracing::warn!(routing_mode, "notifications::resolve_recipients unknown routing mode");
        }
    }

    Ok(RoutingResult {
        recipient_user_ids: user_ids.into_iter().collect(),
        recipient_role_ids: role_ids.into_iter().collect(),
    })
}

fn extract_source(payload: &serde_json::Value) -> Option<(String, i64)> {
    let source_module = payload
        .get("source_module")
        .and_then(serde_json::Value::as_str)?
        .to_string();

    let source_record_id = payload.get("source_record_id").and_then(as_i64)?;
    Some((source_module, source_record_id))
}

fn as_i64(value: &serde_json::Value) -> Option<i64> {
    if let Some(v) = value.as_i64() {
        return Some(v);
    }
    if let Some(v) = value.as_str() {
        return v.parse::<i64>().ok();
    }
    None
}

async fn query_optional_i64<const N: usize>(
    pool: &SqlitePool,
    sql: &str,
    values: [sea_orm::Value; N],
    col: &str,
) -> Result<Option<i64>> {
    let row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<Option<i64>>("", col).ok().flatten()))
}
