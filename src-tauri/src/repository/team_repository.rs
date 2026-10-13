//! Operational maintenance crews (`teams` table) — repository SSOT.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

const TEAM_TYPES: &[&str] = &["maintenance", "inspection", "planning", "contractor", "hse"];
const TEAM_STATUSES: &[&str] = &["active", "inactive", "disbanded"];
const TEAM_ICON_KEYS: &[&str] = &[
    "shield",
    "shield-alert",
    "zap",
    "wrench",
    "users",
    "flame",
    "anchor",
    "cpu",
    "hard-hat",
    "hammer",
    "cog",
    "factory",
];

#[derive(Debug, Clone, Serialize, Deserialize, FromQueryResult)]
pub struct TeamSummary {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub team_type: String,
    pub status: String,
    pub primary_node_id: Option<i64>,
    pub node_name: Option<String>,
    pub node_code: Option<String>,
    pub description: Option<String>,
    pub icon_key: Option<String>,
    pub accent_color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamListFilter {
    pub entity_id: Option<i64>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamCreateInput {
    pub code: String,
    pub name: String,
    pub team_type: String,
    pub primary_node_id: i64,
    pub description: Option<String>,
    pub status: Option<String>,
    pub icon_key: Option<String>,
    pub accent_color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamUpdateInput {
    pub id: i64,
    pub code: Option<String>,
    pub name: Option<String>,
    pub team_type: Option<String>,
    pub primary_node_id: Option<i64>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub icon_key: Option<String>,
    pub accent_color: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct SkillDefinitionRow {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub is_authorization_required: i64,
    pub revalidation_months: i64,
    pub is_active: i64,
}

fn validate_team_type(value: &str) -> AppResult<()> {
    if TEAM_TYPES.contains(&value) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Invalid team_type '{value}'. Allowed: {}.",
            TEAM_TYPES.join(", ")
        )]))
    }
}

fn validate_team_status(value: &str) -> AppResult<()> {
    if TEAM_STATUSES.contains(&value) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Invalid team status '{value}'. Allowed: {}.",
            TEAM_STATUSES.join(", ")
        )]))
    }
}

fn validate_icon_key(value: Option<&str>) -> AppResult<Option<String>> {
    let Some(raw) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if TEAM_ICON_KEYS.contains(&raw) {
        Ok(Some(raw.to_string()))
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Invalid icon_key '{raw}'. Allowed: {}.",
            TEAM_ICON_KEYS.join(", ")
        )]))
    }
}

fn validate_accent_color(value: Option<&str>) -> AppResult<Option<String>> {
    let Some(raw) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let ok = raw.len() == 7 && raw.starts_with('#') && raw.chars().skip(1).all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(Some(raw.to_ascii_lowercase()))
    } else {
        Err(AppError::ValidationFailed(vec![
            "accent_color must be a #RRGGBB hex color.".into(),
        ]))
    }
}

/// Assert an active, non-deleted crew exists.
pub async fn assert_active_crew_exists(db: &impl ConnectionTrait, team_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM teams
             WHERE id = ? AND deleted_at IS NULL AND status = 'active'
             LIMIT 1",
            [team_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Crew (team_id={team_id}) introuvable ou inactif."
        )]));
    }
    Ok(())
}

/// Lists active crews, optionally scoped to an entity (node or descendant).
pub async fn list_active_teams(db: &DatabaseConnection, filter: TeamListFilter) -> AppResult<Vec<TeamSummary>> {
    let include_inactive = filter.include_inactive.unwrap_or(false);
    let status_sql = if include_inactive {
        "t.deleted_at IS NULL AND t.status IN ('active', 'inactive')"
    } else {
        "t.deleted_at IS NULL AND t.status = 'active'"
    };

    let (scope_sql, values): (String, Vec<sea_orm::Value>) = if let Some(entity_id) = filter.entity_id {
        (
            format!(
                "{status_sql}
                 AND t.primary_node_id IS NOT NULL
                 AND (
                   t.primary_node_id = ?
                   OR EXISTS (
                     SELECT 1
                       FROM org_nodes crew_node
                       JOIN org_nodes entity ON entity.id = ?
                      WHERE crew_node.id = t.primary_node_id
                        AND (
                          crew_node.id = entity.id
                          OR crew_node.ancestor_path LIKE entity.ancestor_path || '%'
                        )
                   )
                 )"
            ),
            vec![entity_id.into(), entity_id.into()],
        )
    } else {
        (status_sql.to_string(), Vec::new())
    };

    let sql = format!(
        "SELECT t.id, t.entity_sync_id, t.code, t.name, t.team_type, t.status,
                t.primary_node_id, n.name AS node_name, n.code AS node_code,
                t.description, t.icon_key, t.accent_color
           FROM teams t
           LEFT JOIN org_nodes n ON n.id = t.primary_node_id
          WHERE {scope_sql}
          ORDER BY t.name ASC"
    );

    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, values);
    Ok(TeamSummary::find_by_statement(stmt).all(db).await?)
}

/// Lists all active skill definitions, with category name.
pub async fn list_all_skills(db: &DatabaseConnection) -> AppResult<Vec<SkillDefinitionRow>> {
    let sql = r"
        SELECT s.id, s.entity_sync_id, s.code, s.name, s.category_id,
               c.name AS category_name,
               s.is_authorization_required, s.revalidation_months, s.is_active
        FROM skill_definitions s
        LEFT JOIN skill_categories c ON c.id = s.category_id
        WHERE s.deleted_at IS NULL AND s.is_active = 1
        ORDER BY c.name ASC, s.name ASC
    ";
    let stmt = Statement::from_string(DbBackend::Sqlite, sql.to_string());
    Ok(SkillDefinitionRow::find_by_statement(stmt).all(db).await?)
}

/// Gets a single team by id.
pub async fn get_team_by_id(db: &impl ConnectionTrait, team_id: i64) -> AppResult<TeamSummary> {
    let sql = r"
        SELECT t.id, t.entity_sync_id, t.code, t.name, t.team_type, t.status,
               t.primary_node_id, n.name AS node_name, n.code AS node_code,
               t.description, t.icon_key, t.accent_color
        FROM teams t
        LEFT JOIN org_nodes n ON n.id = t.primary_node_id
        WHERE t.id = ? AND t.deleted_at IS NULL
    ";
    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, [team_id.into()]);
    TeamSummary::find_by_statement(stmt)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "team".into(),
            id: team_id.to_string(),
        })
}

pub async fn create_team(db: &DatabaseConnection, input: TeamCreateInput) -> AppResult<TeamSummary> {
    let code = input.code.trim().to_string();
    let name = input.name.trim().to_string();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec!["Crew code is required.".into()]));
    }
    if name.is_empty() {
        return Err(AppError::ValidationFailed(vec!["Crew name is required.".into()]));
    }
    validate_team_type(input.team_type.trim())?;
    let status = input.status.unwrap_or_else(|| "active".to_string());
    validate_team_status(status.trim())?;
    let icon_key = validate_icon_key(input.icon_key.as_deref())?;
    let accent_color = validate_accent_color(input.accent_color.as_deref())?;
    crate::org::model_scope::assert_org_node_active(db, input.primary_node_id).await?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();

    let txn = db.begin().await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO teams (
            entity_sync_id, code, name, team_type, primary_node_id, description, status,
            icon_key, accent_color,
            created_at, updated_at, row_version
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            entity_sync_id.into(),
            code.into(),
            name.into(),
            input.team_type.trim().to_string().into(),
            input.primary_node_id.into(),
            input.description.clone().into(),
            status.trim().to_string().into(),
            icon_key.into(),
            accent_color.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec!["Crew code already exists.".into()])
        } else {
            AppError::Database(e)
        }
    })?;

    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("team insert missing id")))?;
    let id: i64 = id_row.try_get("", "id").unwrap_or_default();
    let team = get_team_by_id(&txn, id).await?;
    txn.commit().await?;
    Ok(team)
}

pub async fn update_team(db: &DatabaseConnection, input: TeamUpdateInput) -> AppResult<TeamSummary> {
    let existing = get_team_by_id(db, input.id).await?;
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(code) = input.code {
        let trimmed = code.trim().to_string();
        if trimmed.is_empty() {
            return Err(AppError::ValidationFailed(vec!["Crew code is required.".into()]));
        }
        sets.push("code = ?".into());
        values.push(trimmed.into());
    }
    if let Some(name) = input.name {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err(AppError::ValidationFailed(vec!["Crew name is required.".into()]));
        }
        sets.push("name = ?".into());
        values.push(trimmed.into());
    }
    if let Some(team_type) = input.team_type {
        validate_team_type(team_type.trim())?;
        sets.push("team_type = ?".into());
        values.push(team_type.trim().to_string().into());
    }
    if let Some(node_id) = input.primary_node_id {
        crate::org::model_scope::assert_org_node_active(db, node_id).await?;
        sets.push("primary_node_id = ?".into());
        values.push(node_id.into());
    }
    if let Some(description) = input.description {
        sets.push("description = ?".into());
        values.push(description.into());
    }
    if let Some(status) = input.status {
        validate_team_status(status.trim())?;
        sets.push("status = ?".into());
        values.push(status.trim().to_string().into());
    }
    if input.icon_key.is_some() {
        let icon_key = validate_icon_key(input.icon_key.as_deref())?;
        sets.push("icon_key = ?".into());
        values.push(icon_key.into());
    }
    if input.accent_color.is_some() {
        let accent_color = validate_accent_color(input.accent_color.as_deref())?;
        sets.push("accent_color = ?".into());
        values.push(accent_color.into());
    }

    if sets.is_empty() {
        return Ok(existing);
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    sets.push("updated_at = ?".into());
    values.push(now.into());
    sets.push("row_version = row_version + 1".into());
    values.push(input.id.into());

    let sql = format!(
        "UPDATE teams SET {} WHERE id = ? AND deleted_at IS NULL",
        sets.join(", ")
    );
    let txn = db.begin().await?;
    txn.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AppError::ValidationFailed(vec!["Crew code already exists.".into()])
            } else {
                AppError::Database(e)
            }
        })?;

    let team = get_team_by_id(&txn, input.id).await?;
    txn.commit().await?;
    Ok(team)
}

/// Resolve active crew id by business code.
pub async fn try_resolve_active_team_id_by_code(db: &impl ConnectionTrait, code: &str) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM teams
             WHERE UPPER(TRIM(code)) = UPPER(TRIM(?))
               AND deleted_at IS NULL
               AND status = 'active'
             LIMIT 1",
            [code.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i64>("", "id").ok()))
}
