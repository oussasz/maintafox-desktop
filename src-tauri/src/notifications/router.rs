use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::notifications::Result;

#[derive(Debug, Clone, Default)]
pub struct RoutingResult {
    pub recipient_user_ids: Vec<i64>,
    pub recipient_role_ids: Vec<i64>,
}

pub async fn resolve_recipients(
    pool: &impl ConnectionTrait,
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
                        "SELECT pp.assigned_team_id
                         FROM pm_occurrences po
                         JOIN pm_plans pp ON pp.id = po.pm_plan_id
                         WHERE po.id = ?",
                        [source_id.into()],
                        "assigned_team_id",
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
                        user_ids.extend(node_responsible_user_ids(pool, org_node_id).await?);
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
        "submitter" => {
            if let Some((source_module, source_id)) = extract_source(payload) {
                if source_module == "intervention_requests" || source_module == "di" {
                    if let Some(user_id) = query_optional_i64(
                        pool,
                        "SELECT submitter_id FROM intervention_requests WHERE id = ?",
                        [source_id.into()],
                        "submitter_id",
                    )
                    .await?
                    {
                        user_ids.insert(user_id);
                    }
                }
            }
        }
        "role" => {
            if let Some(role_name) = payload.get("routing_role_name").and_then(serde_json::Value::as_str) {
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
                .or_else(|| payload.get("assigned_team_id").and_then(as_i64));
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
            if let Some(watchers) = payload.get("watcher_user_ids").and_then(serde_json::Value::as_array) {
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

/// Active users holding a routing responsibility (maintenance owner, approver, entity manager)
/// on an org node, counting only assignments in force now.
async fn node_responsible_user_ids(pool: &impl ConnectionTrait, org_node_id: i64) -> Result<Vec<i64>> {
    let rows = pool
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ua.id AS user_id
                 FROM org_node_responsibilities r
                 JOIN user_accounts ua ON ua.personnel_id = r.person_id
                 WHERE r.node_id = ?
                   -- Governed codes are seeded uppercase (org.responsibility_type).
                   AND UPPER(r.responsibility_type) IN ('ENTITY_MANAGER', 'MAINTENANCE_OWNER', 'APPROVER')
                   -- Only assignments in force now: ended ones keep their history row.
                   AND (r.valid_from IS NULL OR r.valid_from <= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                   AND (r.valid_to IS NULL OR r.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                   AND ua.is_active = 1",
            [org_node_id.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.try_get::<i64>("", "user_id").ok())
        .collect())
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
    pool: &impl ConnectionTrait,
    sql: &str,
    values: [sea_orm::Value; N],
    col: &str,
) -> Result<Option<i64>> {
    let row = pool
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    Ok(row.and_then(|r| r.try_get::<Option<i64>>("", col).ok().flatten()))
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use super::node_responsible_user_ids;

    #[tokio::test]
    async fn node_responsibilities_route_by_governed_code_and_only_while_in_force() {
        let db = Database::connect("sqlite::memory:").await.expect("connect");
        crate::migrations::Migrator::up(&db, None).await.expect("migrations");
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
