//! Qualification matrix — people × (skills, certifications) with each cell's state, the view a
//! maintenance manager uses to see coverage and what runs out (SAP HCM qualification profile
//! matrix, Maximo labour qualifications).

use std::collections::HashMap;

use chrono::Duration;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

use super::queries::{parse_iso_date, DEFAULT_RENEWAL_LEAD_DAYS};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QualificationMatrixFilter {
    pub team_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub company_id: Option<i64>,
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatrixColumn {
    /// `cert:<id>` | `skill:<id>`.
    pub key: String,
    /// `certification` | `skill`.
    pub kind: String,
    pub id: i64,
    pub code: String,
    pub label: String,
    /// Certification category; `skill` for skills.
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatrixCell {
    pub key: String,
    /// Certifications: `valid` | `expiring_soon` | `expired` | `pending` | `rejected`.
    /// Skills: `valid` | `expired` | `unvalidated`.
    pub status: String,
    pub level: Option<i64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatrixRow {
    pub personnel_id: i64,
    pub full_name: String,
    pub employee_code: String,
    pub employment_type: String,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub position_name: Option<String>,
    pub company_name: Option<String>,
    /// Only the cells the person holds; a column without a cell is "not held".
    pub cells: Vec<MatrixCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationMatrix {
    pub columns: Vec<MatrixColumn>,
    pub rows: Vec<MatrixRow>,
    pub generated_on: String,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("qualification matrix decode '{field}': {err}"))
}

/// State of a certificate on `today` (fails closed: an unreadable expiry is expired).
fn cert_status(
    verification: &str,
    expires_at: Option<&str>,
    lead_days: Option<i64>,
    today: chrono::NaiveDate,
) -> &'static str {
    match verification {
        "rejected" => "rejected",
        "verified" => match expires_at {
            None => "valid",
            Some(raw) => match parse_iso_date(raw) {
                None => "expired",
                Some(exp) if exp < today => "expired",
                Some(exp) if exp <= today + Duration::days(lead_days.unwrap_or(DEFAULT_RENEWAL_LEAD_DAYS).max(0)) => {
                    "expiring_soon"
                }
                Some(_) => "valid",
            },
        },
        _ => "pending",
    }
}

/// Rank used to keep the best certificate when a person holds several of one type.
fn cert_rank(status: &str) -> u8 {
    match status {
        "valid" => 5,
        "expiring_soon" => 4,
        "pending" => 3,
        "expired" => 2,
        _ => 1,
    }
}

pub async fn get_qualification_matrix(
    db: &impl ConnectionTrait,
    filter: QualificationMatrixFilter,
) -> AppResult<QualificationMatrix> {
    let today = crate::clock::now().date_naive();

    let mut columns: Vec<MatrixColumn> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, category FROM certification_types WHERE is_active = 1 \
             ORDER BY category, code"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| {
            let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
            Ok(MatrixColumn {
                key: format!("cert:{id}"),
                kind: "certification".to_string(),
                id,
                code: r.try_get("", "code").map_err(|e| decode_err("code", e))?,
                label: r.try_get("", "name").map_err(|e| decode_err("name", e))?,
                category: r.try_get("", "category").map_err(|e| decode_err("category", e))?,
            })
        })
        .collect::<AppResult<_>>()?;
    let skill_columns: Vec<MatrixColumn> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code, rv.label FROM reference_values rv \
             JOIN reference_sets rs ON rs.id = rv.set_id \
             JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rd.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.is_active = 1 \
             ORDER BY rv.sort_order, rv.label"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| {
            let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
            Ok(MatrixColumn {
                key: format!("skill:{id}"),
                kind: "skill".to_string(),
                id,
                code: r.try_get("", "code").map_err(|e| decode_err("code", e))?,
                label: r.try_get("", "label").map_err(|e| decode_err("label", e))?,
                category: "skill".to_string(),
            })
        })
        .collect::<AppResult<_>>()?;
    columns.extend(skill_columns);

    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if !filter.include_inactive {
        where_sql.push("p.employment_status = 'active'".to_string());
    }
    if let Some(team) = filter.team_id {
        where_sql.push(
            "(p.primary_team_id = ? OR EXISTS (SELECT 1 FROM personnel_team_assignments pta \
              WHERE pta.personnel_id = p.id AND pta.team_id = ? \
                AND (pta.valid_to IS NULL OR date(pta.valid_to) >= date('now'))))"
                .to_string(),
        );
        values.push(team.into());
        values.push(team.into());
    }
    if let Some(entity) = filter.entity_id {
        where_sql.push("p.primary_entity_id = ?".to_string());
        values.push(entity.into());
    }
    if let Some(company) = filter.company_id {
        where_sql.push("p.external_company_id = ?".to_string());
        values.push(company.into());
    }
    let people = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT p.id, p.full_name, p.employee_code, p.employment_type, p.primary_team_id, \
                        t.name AS team_name, pos.name AS position_name, ec.name AS company_name \
                 FROM personnel p \
                 LEFT JOIN teams t ON t.id = p.primary_team_id \
                 LEFT JOIN positions pos ON pos.id = p.position_id \
                 LEFT JOIN external_companies ec ON ec.id = p.external_company_id \
                 WHERE {} ORDER BY p.full_name",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;

    let mut rows: Vec<MatrixRow> = Vec::with_capacity(people.len());
    let mut index: HashMap<i64, usize> = HashMap::new();
    for p in &people {
        let id: i64 = p.try_get("", "id").map_err(|e| decode_err("id", e))?;
        index.insert(id, rows.len());
        rows.push(MatrixRow {
            personnel_id: id,
            full_name: p.try_get("", "full_name").map_err(|e| decode_err("full_name", e))?,
            employee_code: p
                .try_get("", "employee_code")
                .map_err(|e| decode_err("employee_code", e))?,
            employment_type: p
                .try_get("", "employment_type")
                .map_err(|e| decode_err("employment_type", e))?,
            team_id: p.try_get("", "primary_team_id").ok().flatten(),
            team_name: p.try_get("", "team_name").ok().flatten(),
            position_name: p.try_get("", "position_name").ok().flatten(),
            company_name: p.try_get("", "company_name").ok().flatten(),
            cells: Vec::new(),
        });
    }
    if rows.is_empty() {
        return Ok(QualificationMatrix {
            columns,
            rows,
            generated_on: today.to_string(),
        });
    }

    // Best certificate per (person, type).
    let mut best: HashMap<(i64, i64), MatrixCell> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT pc.personnel_id, pc.certification_type_id, pc.expires_at, pc.verification_status, \
                    ct.renewal_lead_days \
             FROM personnel_certifications pc \
             JOIN certification_types ct ON ct.id = pc.certification_type_id AND ct.is_active = 1"
                .to_string(),
        ))
        .await?
    {
        let pid: i64 = r
            .try_get("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?;
        if !index.contains_key(&pid) {
            continue;
        }
        let ct: i64 = r
            .try_get("", "certification_type_id")
            .map_err(|e| decode_err("certification_type_id", e))?;
        let expires_at: Option<String> = r
            .try_get::<Option<String>>("", "expires_at")
            .map_err(|e| decode_err("expires_at", e))?
            .filter(|s| !s.trim().is_empty());
        let verification: String = r
            .try_get("", "verification_status")
            .map_err(|e| decode_err("verification_status", e))?;
        let lead: Option<i64> = r.try_get("", "renewal_lead_days").ok().flatten();
        let status = cert_status(&verification, expires_at.as_deref(), lead, today);
        let cell = MatrixCell {
            key: format!("cert:{ct}"),
            status: status.to_string(),
            level: None,
            expires_at,
        };
        match best.get(&(pid, ct)) {
            Some(existing) if cert_rank(&existing.status) >= cert_rank(status) => {}
            _ => {
                best.insert((pid, ct), cell);
            }
        }
    }
    for ((pid, _), cell) in best {
        rows[index[&pid]].cells.push(cell);
    }

    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT personnel_id, reference_value_id, proficiency_level, valid_to, is_validated \
             FROM personnel_skills"
                .to_string(),
        ))
        .await?
    {
        let pid: i64 = r
            .try_get("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?;
        let Some(&row_index) = index.get(&pid) else {
            continue;
        };
        let skill: i64 = r
            .try_get("", "reference_value_id")
            .map_err(|e| decode_err("reference_value_id", e))?;
        let valid_to: Option<String> = r
            .try_get::<Option<String>>("", "valid_to")
            .map_err(|e| decode_err("valid_to", e))?
            .filter(|s| !s.trim().is_empty());
        let validated: i64 = r
            .try_get("", "is_validated")
            .map_err(|e| decode_err("is_validated", e))?;
        let expired = valid_to
            .as_deref()
            .is_some_and(|v| parse_iso_date(v).is_none_or(|d| d < today));
        let status = if expired {
            "expired"
        } else if validated == 0 {
            "unvalidated"
        } else {
            "valid"
        };
        rows[row_index].cells.push(MatrixCell {
            key: format!("skill:{skill}"),
            status: status.to_string(),
            level: Some(
                r.try_get("", "proficiency_level")
                    .map_err(|e| decode_err("proficiency_level", e))?,
            ),
            expires_at: valid_to,
        });
    }
    for row in &mut rows {
        row.cells.sort_by(|a, b| a.key.cmp(&b.key));
    }

    Ok(QualificationMatrix {
        columns,
        rows,
        generated_on: today.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::cert_status;
    use chrono::NaiveDate;

    #[test]
    fn certificate_states() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        assert_eq!(cert_status("verified", None, None, today), "valid");
        assert_eq!(cert_status("verified", Some("2026-10-13"), None, today), "expired");
        assert_eq!(
            cert_status("verified", Some("2026-10-30"), Some(30), today),
            "expiring_soon"
        );
        assert_eq!(cert_status("verified", Some("2027-06-01"), Some(30), today), "valid");
        assert_eq!(cert_status("verified", Some("not a date"), None, today), "expired");
        assert_eq!(cert_status("pending", Some("2027-06-01"), None, today), "pending");
        assert_eq!(cert_status("rejected", None, None, today), "rejected");
    }
}
