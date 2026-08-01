//! Personnel assignment history queries.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;
use super::domain::PersonnelAssignmentHistoryEntry;

pub async fn list_personnel_assignment_history(
    db: &DatabaseConnection,
    personnel_id: i64,
    limit: i64,
) -> AppResult<Vec<PersonnelAssignmentHistoryEntry>> {
    let rows: Vec<sea_orm::QueryResult> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                h.id, h.personnel_id,
                h.entity_id, h.team_id, h.position_id, h.manager_id,
                h.schedule_reference_value_id,
                h.started_at, h.ended_at, h.reason, h.changed_by_id, h.created_at,
                en.name  AS entity_name,
                tm.name  AS team_name,
                pos.code AS position_code,
                pos.name AS position_name,
                mgr.full_name AS manager_name,
                sched.label   AS schedule_name
             FROM personnel_assignment_history h
             LEFT JOIN org_nodes   en    ON en.id    = h.entity_id
             LEFT JOIN org_nodes   tm    ON tm.id    = h.team_id
             LEFT JOIN positions   pos   ON pos.id   = h.position_id
             LEFT JOIN personnel   mgr   ON mgr.id   = h.manager_id
             LEFT JOIN reference_values sched ON sched.id = h.schedule_reference_value_id
             WHERE h.personnel_id = ?
             ORDER BY h.started_at DESC, h.id DESC
             LIMIT ?",
            [personnel_id.into(), limit.into()],
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|r: sea_orm::QueryResult| PersonnelAssignmentHistoryEntry {
            id: r.try_get("", "id").unwrap_or_default(),
            personnel_id: r.try_get("", "personnel_id").unwrap_or_default(),
            entity_id: r.try_get("", "entity_id").unwrap_or(None),
            team_id: r.try_get("", "team_id").unwrap_or(None),
            position_id: r.try_get("", "position_id").unwrap_or(None),
            manager_id: r.try_get("", "manager_id").unwrap_or(None),
            schedule_reference_value_id: r.try_get("", "schedule_reference_value_id").unwrap_or(None),
            started_at: r.try_get("", "started_at").unwrap_or_default(),
            ended_at: r.try_get("", "ended_at").unwrap_or(None),
            reason: r.try_get("", "reason").unwrap_or(None),
            changed_by_id: r.try_get("", "changed_by_id").unwrap_or(None),
            created_at: r.try_get("", "created_at").unwrap_or_default(),
            entity_name: r.try_get("", "entity_name").unwrap_or(None),
            team_name: r.try_get("", "team_name").unwrap_or(None),
            position_code: r.try_get("", "position_code").unwrap_or(None),
            position_name: r.try_get("", "position_name").unwrap_or(None),
            manager_name: r.try_get("", "manager_name").unwrap_or(None),
            schedule_name: r.try_get("", "schedule_name").unwrap_or(None),
        })
        .collect())
}
