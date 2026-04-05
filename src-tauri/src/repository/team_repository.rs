use crate::errors::{AppError, AppResult};
use sea_orm::{DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::Serialize;

// ── DTOs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct TeamSummary {
    pub id: i32,
    pub sync_id: String,
    pub code: String,
    pub name: String,
    pub team_type: String,
    pub status: String,
    pub primary_node_id: Option<i32>,
    pub node_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct SkillDefinitionRow {
    pub id: i32,
    pub sync_id: String,
    pub code: String,
    pub name: String,
    pub category_id: Option<i32>,
    pub category_name: Option<String>,
    pub is_authorization_required: i32,
    pub revalidation_months: i32,
    pub is_active: i32,
}

// ── Repository functions ──────────────────────────────────────────────────

/// Lists all active teams. Used by work order assignment dropdowns.
pub async fn list_active_teams(db: &DatabaseConnection) -> AppResult<Vec<TeamSummary>> {
    let sql = r#"
        SELECT t.id, t.sync_id, t.code, t.name, t.team_type, t.status,
               t.primary_node_id, n.name AS node_name
        FROM teams t
        LEFT JOIN org_nodes n ON n.id = t.primary_node_id
        WHERE t.deleted_at IS NULL AND t.status = 'active'
        ORDER BY t.name ASC
    "#;
    let stmt = Statement::from_string(DbBackend::Sqlite, sql.to_string());
    Ok(TeamSummary::find_by_statement(stmt).all(db).await?)
}

/// Lists all active skill definitions, with category name.
pub async fn list_all_skills(db: &DatabaseConnection) -> AppResult<Vec<SkillDefinitionRow>> {
    let sql = r#"
        SELECT s.id, s.sync_id, s.code, s.name, s.category_id,
               c.name AS category_name,
               s.is_authorization_required, s.revalidation_months, s.is_active
        FROM skill_definitions s
        LEFT JOIN skill_categories c ON c.id = s.category_id
        WHERE s.deleted_at IS NULL AND s.is_active = 1
        ORDER BY c.name ASC, s.name ASC
    "#;
    let stmt = Statement::from_string(DbBackend::Sqlite, sql.to_string());
    Ok(SkillDefinitionRow::find_by_statement(stmt).all(db).await?)
}

/// Gets a single team by id.
pub async fn get_team_by_id(db: &DatabaseConnection, team_id: i32) -> AppResult<TeamSummary> {
    let sql = r#"
        SELECT t.id, t.sync_id, t.code, t.name, t.team_type, t.status,
               t.primary_node_id, n.name AS node_name
        FROM teams t
        LEFT JOIN org_nodes n ON n.id = t.primary_node_id
        WHERE t.id = ? AND t.deleted_at IS NULL
    "#;
    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, [team_id.into()]);
    TeamSummary::find_by_statement(stmt)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "team".into(),
            id: team_id.to_string(),
        })
}
