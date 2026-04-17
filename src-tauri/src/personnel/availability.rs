//! Personnel availability computations and block mutations.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize)]
pub struct AvailabilityCalendarFilter {
    pub date_from: String,
    pub date_to: String,
    pub personnel_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub team_id: Option<i64>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvailabilityCalendarEntry {
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub entity_id: Option<i64>,
    pub entity_name: Option<String>,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub work_date: String,
    pub shift_start: Option<String>,
    pub shift_end: Option<String>,
    pub scheduled_minutes: i64,
    pub blocked_minutes: i64,
    pub available_minutes: i64,
    pub has_critical_block: bool,
    pub block_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AvailabilityBlockCreateInput {
    pub personnel_id: i64,
    pub block_type: String,
    pub start_at: String,
    pub end_at: String,
    pub reason_note: Option<String>,
    pub is_critical: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonnelAvailabilityBlock {
    pub id: i64,
    pub personnel_id: i64,
    pub block_type: String,
    pub start_at: String,
    pub end_at: String,
    pub reason_note: Option<String>,
    pub is_critical: bool,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

pub async fn list_availability_calendar(
    db: &DatabaseConnection,
    filter: AvailabilityCalendarFilter,
) -> AppResult<Vec<AvailabilityCalendarEntry>> {
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
    if let Some(personnel_id) = filter.personnel_id {
        personnel_where.push("p.id = ?".to_string());
        values.push(personnel_id.into());
    }
    if !filter.include_inactive.unwrap_or(false) {
        personnel_where.push("p.availability_status <> 'inactive'".to_string());
    }

    let team_filter_sql = if filter.team_id.is_some() {
        "AND COALESCE(t.id, pd.primary_team_id) = ?"
    } else {
        ""
    };
    if let Some(team_id) = filter.team_id {
        values.push(team_id.into());
    }

    let sql = format!(
        "WITH RECURSIVE dates(work_date) AS (
            SELECT date(?)
            UNION ALL
            SELECT date(work_date, '+1 day') FROM dates WHERE work_date < date(?)
         ),
         base_personnel AS (
            SELECT
                p.id,
                p.employee_code,
                p.full_name,
                p.primary_entity_id,
                p.primary_team_id,
                p.home_schedule_id
            FROM personnel p
            WHERE {}
         ),
         person_days AS (
            SELECT
                bp.*,
                d.work_date,
                ((CAST(strftime('%w', d.work_date) AS INTEGER) + 6) % 7) + 1 AS day_of_week
            FROM base_personnel bp
            CROSS JOIN dates d
         )
         SELECT
            pd.id AS personnel_id,
            pd.employee_code,
            pd.full_name,
            pd.primary_entity_id AS entity_id,
            e.name AS entity_name,
            COALESCE(t.id, pd.primary_team_id) AS team_id,
            COALESCE(t.name, pt.name) AS team_name,
            pd.work_date,
            sd.shift_start,
            sd.shift_end,
            CASE
                WHEN sd.id IS NULL OR sd.is_rest_day = 1 THEN 0
                ELSE MAX(CAST((julianday(pd.work_date || 'T' || sd.shift_end || ':00Z') - julianday(pd.work_date || 'T' || sd.shift_start || ':00Z')) * 1440 AS INTEGER), 0)
            END AS scheduled_minutes,
            COALESCE((
                SELECT SUM(MAX(CAST((julianday(MIN(ab.end_at, pd.work_date || 'T' || sd.shift_end || ':00Z')) - julianday(MAX(ab.start_at, pd.work_date || 'T' || sd.shift_start || ':00Z'))) * 1440 AS INTEGER), 0))
                FROM personnel_availability_blocks ab
                WHERE ab.personnel_id = pd.id
                  AND sd.id IS NOT NULL
                  AND sd.is_rest_day = 0
                  AND ab.start_at < pd.work_date || 'T' || sd.shift_end || ':00Z'
                  AND ab.end_at > pd.work_date || 'T' || sd.shift_start || ':00Z'
            ), 0) AS blocked_minutes,
            CASE WHEN EXISTS (
                SELECT 1 FROM personnel_availability_blocks ab2
                WHERE ab2.personnel_id = pd.id
                  AND ab2.is_critical = 1
                  AND sd.id IS NOT NULL
                  AND sd.is_rest_day = 0
                  AND ab2.start_at < pd.work_date || 'T' || sd.shift_end || ':00Z'
                  AND ab2.end_at > pd.work_date || 'T' || sd.shift_start || ':00Z'
            ) THEN 1 ELSE 0 END AS has_critical_block,
            COALESCE((
                SELECT group_concat(DISTINCT ab3.block_type)
                FROM personnel_availability_blocks ab3
                WHERE ab3.personnel_id = pd.id
                  AND sd.id IS NOT NULL
                  AND sd.is_rest_day = 0
                  AND ab3.start_at < pd.work_date || 'T' || sd.shift_end || ':00Z'
                  AND ab3.end_at > pd.work_date || 'T' || sd.shift_start || ':00Z'
            ), '') AS block_types
         FROM person_days pd
         LEFT JOIN org_nodes e ON e.id = pd.primary_entity_id
         LEFT JOIN org_nodes pt ON pt.id = pd.primary_team_id
         LEFT JOIN teams t ON t.id = (
            SELECT pta.team_id
            FROM personnel_team_assignments pta
            WHERE pta.personnel_id = pd.id
              AND (pta.valid_from IS NULL OR date(pta.valid_from) <= pd.work_date)
              AND (pta.valid_to IS NULL OR date(pta.valid_to) >= pd.work_date)
            ORDER BY pta.is_lead DESC, pta.id DESC
            LIMIT 1
         )
         LEFT JOIN schedule_details sd
            ON sd.schedule_class_id = pd.home_schedule_id
           AND sd.day_of_week = pd.day_of_week
         WHERE 1 = 1
         {}
         ORDER BY pd.full_name ASC, pd.work_date ASC",
        personnel_where.join(" AND "),
        team_filter_sql
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
        .map(|row| {
            let scheduled_minutes: i64 = row.try_get("", "scheduled_minutes").unwrap_or_default();
            let blocked_minutes: i64 = row.try_get("", "blocked_minutes").unwrap_or_default();
            let available_minutes = (scheduled_minutes - blocked_minutes).max(0);
            let raw_types: String = row.try_get("", "block_types").unwrap_or_default();
            let block_types = if raw_types.is_empty() {
                Vec::new()
            } else {
                raw_types
                    .split(',')
                    .map(std::string::ToString::to_string)
                    .collect()
            };

            AvailabilityCalendarEntry {
                personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
                employee_code: row.try_get("", "employee_code").unwrap_or_default(),
                full_name: row.try_get("", "full_name").unwrap_or_default(),
                entity_id: row.try_get("", "entity_id").unwrap_or(None),
                entity_name: row.try_get("", "entity_name").unwrap_or(None),
                team_id: row.try_get("", "team_id").unwrap_or(None),
                team_name: row.try_get("", "team_name").unwrap_or(None),
                work_date: row.try_get("", "work_date").unwrap_or_default(),
                shift_start: row.try_get("", "shift_start").unwrap_or(None),
                shift_end: row.try_get("", "shift_end").unwrap_or(None),
                scheduled_minutes,
                blocked_minutes,
                available_minutes,
                has_critical_block: row.try_get::<i64>("", "has_critical_block").unwrap_or(0) == 1,
                block_types,
            }
        })
        .collect();

    Ok(mapped)
}

pub async fn create_availability_block(
    db: &DatabaseConnection,
    input: AvailabilityBlockCreateInput,
    actor_id: i64,
) -> AppResult<PersonnelAvailabilityBlock> {
    if input.start_at >= input.end_at {
        return Err(AppError::ValidationFailed(vec![
            "start_at must be before end_at.".to_string(),
        ]));
    }
    if input.block_type.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "block_type is required.".to_string(),
        ]));
    }

    let is_critical = input.is_critical.unwrap_or(false)
        || matches!(input.block_type.as_str(), "medical" | "restriction");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_availability_blocks
            (personnel_id, block_type, start_at, end_at, reason_note, is_critical, created_by_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            input.personnel_id.into(),
            input.block_type.clone().into(),
            input.start_at.clone().into(),
            input.end_at.clone().into(),
            input.reason_note.clone().into(),
            (if is_critical { 1 } else { 0 }).into(),
            actor_id.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                id,
                personnel_id,
                block_type,
                start_at,
                end_at,
                reason_note,
                is_critical,
                created_by_id,
                created_at
             FROM personnel_availability_blocks
             WHERE id = last_insert_rowid()"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("availability block insert failed")))?;

    Ok(PersonnelAvailabilityBlock {
        id: row.try_get("", "id").unwrap_or_default(),
        personnel_id: row.try_get("", "personnel_id").unwrap_or_default(),
        block_type: row.try_get("", "block_type").unwrap_or_default(),
        start_at: row.try_get("", "start_at").unwrap_or_default(),
        end_at: row.try_get("", "end_at").unwrap_or_default(),
        reason_note: row.try_get("", "reason_note").unwrap_or(None),
        is_critical: row.try_get::<i64>("", "is_critical").unwrap_or(0) == 1,
        created_by_id: row.try_get("", "created_by_id").unwrap_or(None),
        created_at: row.try_get("", "created_at").unwrap_or_default(),
    })
}


