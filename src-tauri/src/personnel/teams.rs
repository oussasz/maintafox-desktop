//! Team capacity summary computations.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize)]
pub struct TeamCapacityFilter {
    pub date_from: String,
    pub date_to: String,
    pub entity_id: Option<i64>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamCapacitySummaryRow {
    pub team_id: i64,
    pub team_code: String,
    pub team_name: String,
    pub member_count: i64,
    pub lead_count: i64,
    pub total_scheduled_minutes: i64,
    pub total_available_minutes: i64,
    pub total_blocked_minutes: i64,
    pub avg_availability_ratio: f64,
}

/// Per team over a range: members, leads, and the scheduled / at-work / absent minutes from
/// `personnel::capacity` (members' Classes horaires on the site's working days, minus absences) —
/// the same figures as planning and the team's capacity page.
pub async fn list_team_capacity_summary(
    db: &DatabaseConnection,
    filter: TeamCapacityFilter,
) -> AppResult<Vec<TeamCapacitySummaryRow>> {
    let parse = |raw: &str, field: &str| {
        NaiveDate::parse_from_str(raw.get(..10).unwrap_or(raw), "%Y-%m-%d").map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "GATE_PERS_DATE_FORMAT:field={field}: {field} must be YYYY-MM-DD."
            )])
        })
    };
    let from = parse(&filter.date_from, "date_from")?;
    let to = parse(&filter.date_to, "date_to")?;
    if from > to {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PERS_DATE_RANGE_ORDER: date_from must be <= date_to.".to_string(),
        ]));
    }

    let mut sql = String::from(
        "SELECT t.id, t.code, t.name, (SELECT COUNT(*) FROM personnel_team_assignments pta2 WHERE pta2.team_id = t.id AND pta2.is_lead = 1 AND (pta2.valid_from IS NULL OR date(pta2.valid_from) <= date('now')) AND (pta2.valid_to IS NULL OR date(pta2.valid_to) >= date('now'))) AS lead_count FROM teams t WHERE t.deleted_at IS NULL",
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if !filter.include_inactive.unwrap_or(false) {
        sql.push_str(" AND t.status = 'active'");
    }
    if let Some(entity_id) = filter.entity_id {
        sql.push_str(" AND (t.primary_node_id = ? OR EXISTS (SELECT 1 FROM personnel p WHERE p.primary_entity_id = ? AND (p.primary_team_id = t.id OR EXISTS (SELECT 1 FROM personnel_team_assignments x WHERE x.personnel_id = p.id AND x.team_id = t.id))))");
        values.push(entity_id.into());
        values.push(entity_id.into());
    }
    sql.push_str(" ORDER BY t.name ASC");
    let teams = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;

    let mut out = Vec::with_capacity(teams.len());
    for t in teams {
        let team_id: i64 = t.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let days = crate::personnel::capacity::team_capacity_span(db, team_id, from, to).await?;
        let members: BTreeSet<i64> = days
            .iter()
            .flat_map(|d| d.members.iter().map(|m| m.personnel_id))
            .collect();
        let minutes = |h: f64| (h * 60.0).round() as i64;
        let scheduled: i64 = days.iter().map(|d| minutes(d.scheduled_hours)).sum();
        let available: i64 = days.iter().map(|d| minutes(d.attendance_hours)).sum();
        let absent: i64 = days.iter().map(|d| minutes(d.absent_hours)).sum();
        out.push(TeamCapacitySummaryRow {
            team_id,
            team_code: t.try_get("", "code").map_err(|e| decode_err("code", e))?,
            team_name: t.try_get("", "name").map_err(|e| decode_err("name", e))?,
            member_count: i64::try_from(members.len()).unwrap_or(i64::MAX),
            lead_count: t.try_get("", "lead_count").map_err(|e| decode_err("lead_count", e))?,
            total_scheduled_minutes: scheduled,
            total_available_minutes: available,
            total_blocked_minutes: absent,
            avg_availability_ratio: if scheduled == 0 {
                0.0
            } else {
                available as f64 / scheduled as f64
            },
        });
    }
    Ok(out)
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("team capacity decode '{field}': {err}"))
}
