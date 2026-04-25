//! Team capacity summary computations.

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

pub async fn list_team_capacity_summary(
    db: &DatabaseConnection,
    filter: TeamCapacityFilter,
) -> AppResult<Vec<TeamCapacitySummaryRow>> {
    if filter.date_from > filter.date_to {
        return Err(AppError::ValidationFailed(vec![
            "date_from must be <= date_to.".to_string(),
        ]));
    }

    let mut personnel_where = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = vec![filter.date_from.into(), filter.date_to.into()];

    if let Some(entity_id) = filter.entity_id {
        personnel_where.push("p.primary_entity_id = ?".to_string());
        values.push(entity_id.into());
    }
    if !filter.include_inactive.unwrap_or(false) {
        personnel_where.push("p.availability_status <> 'inactive'".to_string());
    }

    let sql = format!(
        "WITH RECURSIVE dates(work_date) AS (
            SELECT date(?)
            UNION ALL
            SELECT date(work_date, '+1 day') FROM dates WHERE work_date < date(?)
         ),
         base_personnel AS (
            SELECT p.id, p.primary_team_id, p.home_schedule_id
            FROM personnel p
            WHERE {}
         ),
         person_days AS (
            SELECT
                bp.id AS personnel_id,
                bp.primary_team_id,
                bp.home_schedule_id,
                d.work_date,
                ((CAST(strftime('%w', d.work_date) AS INTEGER) + 6) % 7) + 1 AS day_of_week
            FROM base_personnel bp
            CROSS JOIN dates d
         ),
         availability_by_person_day AS (
            SELECT
                pd.personnel_id,
                pd.work_date,
                COALESCE(
                    (
                        SELECT pta.team_id
                        FROM personnel_team_assignments pta
                        WHERE pta.personnel_id = pd.personnel_id
                          AND (pta.valid_from IS NULL OR date(pta.valid_from) <= pd.work_date)
                          AND (pta.valid_to IS NULL OR date(pta.valid_to) >= pd.work_date)
                        ORDER BY pta.is_lead DESC, pta.id DESC
                        LIMIT 1
                    ),
                    pd.primary_team_id
                ) AS team_id,
                CASE
                    WHEN sd.id IS NULL OR sd.is_rest_day = 1 THEN 0
                    ELSE MAX(CAST((julianday(pd.work_date || 'T' || sd.shift_end || ':00Z') - julianday(pd.work_date || 'T' || sd.shift_start || ':00Z')) * 1440 AS INTEGER), 0)
                END AS scheduled_minutes,
                COALESCE((
                    SELECT SUM(MAX(CAST((julianday(MIN(ab.end_at, pd.work_date || 'T' || sd.shift_end || ':00Z')) - julianday(MAX(ab.start_at, pd.work_date || 'T' || sd.shift_start || ':00Z'))) * 1440 AS INTEGER), 0))
                    FROM personnel_availability_blocks ab
                    WHERE ab.personnel_id = pd.personnel_id
                      AND sd.id IS NOT NULL
                      AND sd.is_rest_day = 0
                      AND ab.start_at < pd.work_date || 'T' || sd.shift_end || ':00Z'
                      AND ab.end_at > pd.work_date || 'T' || sd.shift_start || ':00Z'
                ), 0) AS blocked_minutes
            FROM person_days pd
            LEFT JOIN schedule_details sd
              ON sd.schedule_class_id = pd.home_schedule_id
             AND sd.day_of_week = pd.day_of_week
         )
         SELECT
            t.id AS team_id,
            t.code AS team_code,
            t.name AS team_name,
            COUNT(DISTINCT apd.personnel_id) AS member_count,
            (
                SELECT COUNT(*)
                FROM personnel_team_assignments pta2
                WHERE pta2.team_id = t.id
                  AND pta2.is_lead = 1
                  AND (pta2.valid_from IS NULL OR date(pta2.valid_from) <= date('now'))
                  AND (pta2.valid_to IS NULL OR date(pta2.valid_to) >= date('now'))
            ) AS lead_count,
            SUM(apd.scheduled_minutes) AS total_scheduled_minutes,
            SUM(MAX(apd.scheduled_minutes - apd.blocked_minutes, 0)) AS total_available_minutes,
            SUM(apd.blocked_minutes) AS total_blocked_minutes,
            CASE
                WHEN SUM(apd.scheduled_minutes) = 0 THEN 0.0
                ELSE CAST(SUM(MAX(apd.scheduled_minutes - apd.blocked_minutes, 0)) AS REAL) / CAST(SUM(apd.scheduled_minutes) AS REAL)
            END AS avg_availability_ratio
         FROM availability_by_person_day apd
         JOIN teams t ON t.id = apd.team_id
         GROUP BY t.id, t.code, t.name
         ORDER BY t.name ASC",
        personnel_where.join(" AND ")
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
        .map(|row| TeamCapacitySummaryRow {
            team_id: row.try_get("", "team_id").unwrap_or_default(),
            team_code: row.try_get("", "team_code").unwrap_or_default(),
            team_name: row.try_get("", "team_name").unwrap_or_default(),
            member_count: row.try_get("", "member_count").unwrap_or_default(),
            lead_count: row.try_get("", "lead_count").unwrap_or_default(),
            total_scheduled_minutes: row.try_get("", "total_scheduled_minutes").unwrap_or_default(),
            total_available_minutes: row.try_get("", "total_available_minutes").unwrap_or_default(),
            total_blocked_minutes: row.try_get("", "total_blocked_minutes").unwrap_or_default(),
            avg_availability_ratio: row.try_get("", "avg_availability_ratio").unwrap_or(0.0),
        })
        .collect();

    Ok(mapped)
}


