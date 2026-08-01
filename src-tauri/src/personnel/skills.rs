//! Personnel skills matrix computations.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SkillsMatrixFilter {
    pub personnel_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub team_id: Option<i64>,
    pub skill_code: Option<String>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillMatrixRow {
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub employment_type: String,
    pub availability_status: String,
    pub entity_id: Option<i64>,
    pub entity_name: Option<String>,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub skill_code: Option<String>,
    pub skill_label: Option<String>,
    pub proficiency_level: Option<i64>,
    pub coverage_status: String,
}

pub async fn list_skills_matrix(
    db: &DatabaseConnection,
    filter: SkillsMatrixFilter,
) -> AppResult<Vec<SkillMatrixRow>> {
    let mut where_sql = vec!["(rd.code = 'PERSONNEL.SKILLS' OR rd.code IS NULL)".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(entity_id) = filter.entity_id {
        where_sql.push("p.primary_entity_id = ?".to_string());
        values.push(entity_id.into());
    }
    if let Some(personnel_id) = filter.personnel_id {
        where_sql.push("p.id = ?".to_string());
        values.push(personnel_id.into());
    }
    if let Some(team_id) = filter.team_id {
        where_sql.push("COALESCE(t.id, p.primary_team_id) = ?".to_string());
        values.push(team_id.into());
    }
    if let Some(skill_code) = filter.skill_code {
        where_sql.push("rv.code = ?".to_string());
        values.push(skill_code.into());
    }
    if !filter.include_inactive.unwrap_or(false) {
        where_sql.push("p.availability_status <> 'inactive'".to_string());
    }

    let sql = format!(
        "SELECT
            p.id AS personnel_id,
            p.employee_code,
            p.full_name,
            p.employment_type,
            p.availability_status,
            p.primary_entity_id AS entity_id,
            e.name AS entity_name,
            COALESCE(t.id, p.primary_team_id) AS team_id,
            COALESCE(t.name, pt.name) AS team_name,
            rv.code AS skill_code,
            rv.label AS skill_label,
            ps.proficiency_level,
            CASE
                WHEN ps.id IS NULL THEN 'missing'
                WHEN ps.valid_to IS NOT NULL AND date(ps.valid_to) < date('now') THEN 'expired'
                ELSE 'active'
            END AS coverage_status
         FROM personnel p
         LEFT JOIN org_nodes e ON e.id = p.primary_entity_id
         LEFT JOIN org_nodes pt ON pt.id = p.primary_team_id
         LEFT JOIN teams t ON t.id = (
             SELECT pta.team_id
               FROM personnel_team_assignments pta
              WHERE pta.personnel_id = p.id
                AND (pta.valid_from IS NULL OR date(pta.valid_from) <= date('now'))
                AND (pta.valid_to IS NULL OR date(pta.valid_to) >= date('now'))
              ORDER BY pta.is_lead DESC, pta.id DESC
              LIMIT 1
         )
         LEFT JOIN personnel_skills ps ON ps.personnel_id = p.id
         LEFT JOIN reference_values rv ON rv.id = ps.reference_value_id
         LEFT JOIN reference_sets rs ON rs.id = rv.set_id
         LEFT JOIN reference_domains rd ON rd.id = rs.domain_id
         WHERE {}
         ORDER BY p.full_name ASC, rv.label ASC",
        where_sql.join(" AND ")
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
        .await?;

    let mapped = rows
        .into_iter()
        .map(|row| SkillMatrixRow {
            personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
            employee_code: row.try_get("", "employee_code").unwrap_or_default(),
            full_name: row.try_get("", "full_name").unwrap_or_default(),
            employment_type: row.try_get("", "employment_type").unwrap_or_default(),
            availability_status: row.try_get("", "availability_status").unwrap_or_default(),
            entity_id: row.try_get("", "entity_id").unwrap_or(None),
            entity_name: row.try_get("", "entity_name").unwrap_or(None),
            team_id: row.try_get("", "team_id").unwrap_or(None),
            team_name: row.try_get("", "team_name").unwrap_or(None),
            skill_code: row.try_get("", "skill_code").unwrap_or(None),
            skill_label: row.try_get("", "skill_label").unwrap_or(None),
            proficiency_level: row.try_get("", "proficiency_level").unwrap_or(None),
            coverage_status: row
                .try_get("", "coverage_status")
                .unwrap_or_else(|_| "missing".to_string()),
        })
        .collect();

    Ok(mapped)
}

pub async fn declare_personnel_skill(
    db: &DatabaseConnection,
    personnel_id: i64,
    reference_value_id: i64,
    proficiency_level: i64,
    valid_to: Option<String>,
    _note: Option<String>,
    is_primary: bool,
) -> AppResult<()> {
    let normalized_level = proficiency_level.clamp(1, 5);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_skills
            (personnel_id, reference_value_id, proficiency_level, valid_from, valid_to, source_type, is_primary, created_at, updated_at)
         VALUES
            (?, ?, ?, date('now'), ?, 'self_declared', ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(personnel_id, reference_value_id) DO UPDATE SET
            proficiency_level = excluded.proficiency_level,
            valid_to = excluded.valid_to,
            source_type = 'self_declared',
            is_primary = excluded.is_primary,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        [
            personnel_id.into(),
            reference_value_id.into(),
            normalized_level.into(),
            valid_to.into(),
            (if is_primary { 1 } else { 0 }).into(),
        ],
    ))
    .await?;

    Ok(())
}


