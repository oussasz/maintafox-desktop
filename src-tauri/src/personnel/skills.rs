//! Personnel skills matrix computations.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

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

pub async fn list_skills_matrix(db: &DatabaseConnection, filter: SkillsMatrixFilter) -> AppResult<Vec<SkillMatrixRow>> {
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
        where_sql.push("COALESCE(t.id, home.id, p.primary_team_id) = ?".to_string());
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
            COALESCE(t.id, home.id, p.primary_team_id) AS team_id,
            COALESCE(t.name, home.name) AS team_name,
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
         LEFT JOIN teams home ON home.id = p.primary_team_id
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
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
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
            -- A changed level or validity is a new claim: it needs validating again.
            is_validated = CASE WHEN proficiency_level = excluded.proficiency_level
                                 AND COALESCE(valid_to, '') = COALESCE(excluded.valid_to, '')
                                THEN is_validated ELSE 0 END,
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

// ═══════════════════════════════════════════════════════════════════════════════
// Skills recorded by a manager / trainer
// ═══════════════════════════════════════════════════════════════════════════════

/// One skill of one person, as the person's sheet shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelSkillRow {
    pub id: i64,
    pub personnel_id: i64,
    pub reference_value_id: i64,
    pub skill_code: String,
    pub skill_label: String,
    pub proficiency_level: i64,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    /// `manual` | `self_declared` | `training` | `import`.
    pub source_type: String,
    pub is_primary: bool,
    pub is_validated: bool,
    pub verified_by_name: Option<String>,
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonnelSkillUpsertInput {
    pub personnel_id: i64,
    pub reference_value_id: i64,
    pub proficiency_level: i64,
    pub valid_to: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    /// Mark the skill as checked by the person recording it.
    #[serde(default)]
    pub validate: bool,
}

fn skill_decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("skill decode '{field}': {e}"))
}

pub async fn list_personnel_skills(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<Vec<PersonnelSkillRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ps.id, ps.personnel_id, ps.reference_value_id, rv.code, rv.label, ps.proficiency_level, \
                    ps.valid_from, ps.valid_to, ps.source_type, ps.is_primary, ps.is_validated, \
                    v.full_name AS verified_by_name, ps.verified_at \
             FROM personnel_skills ps \
             JOIN reference_values rv ON rv.id = ps.reference_value_id \
             LEFT JOIN personnel v ON v.id = ps.verified_by_id \
             WHERE ps.personnel_id = ? \
             ORDER BY ps.is_primary DESC, rv.label",
            [personnel_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PersonnelSkillRow {
                id: r.try_get("", "id").map_err(|e| skill_decode_err("id", e))?,
                personnel_id: r
                    .try_get("", "personnel_id")
                    .map_err(|e| skill_decode_err("personnel_id", e))?,
                reference_value_id: r
                    .try_get("", "reference_value_id")
                    .map_err(|e| skill_decode_err("reference_value_id", e))?,
                skill_code: r.try_get("", "code").map_err(|e| skill_decode_err("code", e))?,
                skill_label: r.try_get("", "label").map_err(|e| skill_decode_err("label", e))?,
                proficiency_level: r
                    .try_get("", "proficiency_level")
                    .map_err(|e| skill_decode_err("proficiency_level", e))?,
                valid_from: r
                    .try_get("", "valid_from")
                    .map_err(|e| skill_decode_err("valid_from", e))?,
                valid_to: r.try_get("", "valid_to").map_err(|e| skill_decode_err("valid_to", e))?,
                source_type: r
                    .try_get("", "source_type")
                    .map_err(|e| skill_decode_err("source_type", e))?,
                is_primary: r
                    .try_get::<i64>("", "is_primary")
                    .map_err(|e| skill_decode_err("is_primary", e))?
                    != 0,
                is_validated: r
                    .try_get::<i64>("", "is_validated")
                    .map_err(|e| skill_decode_err("is_validated", e))?
                    != 0,
                verified_by_name: r.try_get("", "verified_by_name").ok().flatten(),
                verified_at: r.try_get("", "verified_at").ok().flatten(),
            })
        })
        .collect()
}

fn skill_issue(code: &str, message: &str) -> AppError {
    crate::errors::org_validation_failed(code, message, std::collections::HashMap::new())
}

/// Records (or updates) a person's skill. `validator_personnel_id` is the personnel record of
/// the user validating it (required when `input.validate`).
pub async fn upsert_personnel_skill(
    db: &impl ConnectionTrait,
    input: PersonnelSkillUpsertInput,
    validator_personnel_id: Option<i64>,
) -> AppResult<Vec<PersonnelSkillRow>> {
    if !(1..=5).contains(&input.proficiency_level) {
        return Err(skill_issue(
            "SKILL_LEVEL_RANGE",
            "Choose a level from 1 (notions) to 5 (expert).",
        ));
    }
    let valid_to = input
        .valid_to
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    if let Some(v) = &valid_to {
        if chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() {
            return Err(skill_issue(
                "SKILL_VALID_TO_INVALID",
                "Enter the end of validity as YYYY-MM-DD.",
            ));
        }
    }
    let in_catalogue = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rv.id = ? AND rd.code = 'PERSONNEL.SKILLS'",
            [input.reference_value_id.into()],
        ))
        .await?
        .is_some();
    if !in_catalogue {
        return Err(skill_issue(
            "REQUIREMENT_SKILL_UNKNOWN",
            "This skill is not in the skills catalogue.",
        ));
    }
    if input.validate && validator_personnel_id.is_none() {
        return Err(skill_issue(
            "SKILL_VALIDATOR_NOT_LINKED",
            "Only a user linked to a personnel record can validate a skill.",
        ));
    }
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let validated_by = if input.validate { validator_personnel_id } else { None };
    let validated_at = validated_by.map(|_| now.clone());
    if input.is_primary {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE personnel_skills SET is_primary = 0, updated_at = ? \
             WHERE personnel_id = ? AND reference_value_id <> ? AND is_primary = 1",
            [
                now.clone().into(),
                input.personnel_id.into(),
                input.reference_value_id.into(),
            ],
        ))
        .await?;
    }
    // A changed level or validity is a new claim: an earlier validation no longer applies.
    let unchanged = "proficiency_level = excluded.proficiency_level \
                     AND COALESCE(valid_to, '') = COALESCE(excluded.valid_to, '')";
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "INSERT INTO personnel_skills \
                (entity_sync_id, personnel_id, reference_value_id, proficiency_level, valid_from, valid_to, source_type, \
                 is_primary, is_validated, verified_by_id, verified_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, date('now'), ?, 'manual', ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(personnel_id, reference_value_id) DO UPDATE SET \
                is_validated = CASE WHEN excluded.is_validated = 1 THEN 1 \
                                    WHEN {unchanged} THEN is_validated ELSE 0 END, \
                verified_by_id = CASE WHEN excluded.is_validated = 1 THEN excluded.verified_by_id \
                                      WHEN {unchanged} THEN verified_by_id ELSE NULL END, \
                verified_at = CASE WHEN excluded.is_validated = 1 THEN excluded.verified_at \
                                   WHEN {unchanged} THEN verified_at ELSE NULL END, \
                proficiency_level = excluded.proficiency_level, \
                valid_to = excluded.valid_to, \
                source_type = 'manual', \
                is_primary = excluded.is_primary, \
                updated_at = excluded.updated_at"
        ),
        [
            crate::sync::identity::new_entity_sync_id().into(),
            input.personnel_id.into(),
            input.reference_value_id.into(),
            input.proficiency_level.into(),
            valid_to.into(),
            i64::from(input.is_primary).into(),
            i64::from(validated_by.is_some()).into(),
            validated_by.into(),
            validated_at.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    list_personnel_skills(db, input.personnel_id).await
}

/// Removes a skill from a person's sheet (replicates as a delete).
pub async fn remove_personnel_skill(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    reference_value_id: i64,
) -> AppResult<Vec<PersonnelSkillRow>> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM personnel_skills WHERE personnel_id = ? AND reference_value_id = ?",
        [personnel_id.into(), reference_value_id.into()],
    ))
    .await?;
    list_personnel_skills(db, personnel_id).await
}
