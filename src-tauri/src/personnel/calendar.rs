//! Plant holiday calendar (`work_calendar_holidays`): days nobody on the calendar works, for
//! every site or for one organisation node and everything under it.

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkCalendarHoliday {
    pub id: i64,
    pub entity_sync_id: String,
    pub holiday_date: String,
    pub label: String,
    pub entity_id: Option<i64>,
    pub entity_name: Option<String>,
    pub is_active: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkCalendarHolidayFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkCalendarHolidayUpsertInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub holiday_date: String,
    pub label: String,
    pub entity_id: Option<i64>,
    pub is_active: Option<bool>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("holiday decode '{field}': {err}"))
}

fn issue(code: &str, message: &str) -> AppError {
    org_validation_failed(code, message, HashMap::new())
}

const SELECT: &str = "SELECT h.id, h.entity_sync_id, h.holiday_date, h.label, h.entity_id, n.name AS entity_name, \
        h.is_active, h.row_version \
     FROM work_calendar_holidays h LEFT JOIN org_nodes n ON n.id = h.entity_id";

fn map_row(r: &sea_orm::QueryResult) -> AppResult<WorkCalendarHoliday> {
    Ok(WorkCalendarHoliday {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        holiday_date: r.try_get("", "holiday_date").map_err(|e| decode_err("holiday_date", e))?,
        label: r.try_get("", "label").map_err(|e| decode_err("label", e))?,
        entity_id: r.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
        entity_name: r.try_get("", "entity_name").ok().flatten(),
        is_active: r
            .try_get::<i64>("", "is_active")
            .map_err(|e| decode_err("is_active", e))?
            != 0,
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn list_holidays(
    db: &impl ConnectionTrait,
    filter: WorkCalendarHolidayFilter,
) -> AppResult<Vec<WorkCalendarHoliday>> {
    let mut sql = format!("{SELECT} WHERE 1 = 1");
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if !filter.include_inactive {
        sql.push_str(" AND h.is_active = 1");
    }
    if let Some(from) = filter.date_from {
        sql.push_str(" AND h.holiday_date >= ?");
        values.push(from.into());
    }
    if let Some(to) = filter.date_to {
        sql.push_str(" AND h.holiday_date <= ?");
        values.push(to.into());
    }
    sql.push_str(" ORDER BY h.holiday_date, h.entity_id");
    db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .iter()
        .map(map_row)
        .collect()
}

async fn get_holiday(db: &impl ConnectionTrait, id: i64) -> AppResult<WorkCalendarHoliday> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SELECT} WHERE h.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkCalendarHoliday".into(),
            id: id.to_string(),
        })?;
    map_row(&row)
}

pub async fn upsert_holiday(
    db: &DatabaseConnection,
    input: WorkCalendarHolidayUpsertInput,
    actor_id: i64,
) -> AppResult<WorkCalendarHoliday> {
    let day = NaiveDate::parse_from_str(input.holiday_date.trim(), "%Y-%m-%d")
        .map_err(|_| issue("HOLIDAY_DATE_INVALID", "Enter the date as YYYY-MM-DD."))?;
    let label = input.label.trim().to_string();
    if label.is_empty() {
        return Err(issue("HOLIDAY_LABEL_REQUIRED", "Name the holiday."));
    }
    let is_active = input.is_active.unwrap_or(true);
    let txn = db.begin().await?;
    if let Some(entity_id) = input.entity_id {
        if txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM org_nodes WHERE id = ?",
                [entity_id.into()],
            ))
            .await?
            .is_none()
        {
            return Err(AppError::NotFound {
                entity: "OrgNode".into(),
                id: entity_id.to_string(),
            });
        }
    }
    if is_active
        && txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM work_calendar_holidays \
                 WHERE is_active = 1 AND holiday_date = ? AND COALESCE(entity_id, 0) = COALESCE(?, 0) AND id <> ?",
                [
                    day.to_string().into(),
                    input.entity_id.into(),
                    input.id.unwrap_or(0).into(),
                ],
            ))
            .await?
            .is_some()
    {
        return Err(issue(
            "HOLIDAY_DUPLICATE",
            "This day is already a holiday for this site.",
        ));
    }
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let id = if let Some(id) = input.id {
        let current = get_holiday(&txn, id).await?;
        let res = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_calendar_holidays SET holiday_date = ?, label = ?, entity_id = ?, is_active = ?, \
                 row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
                [
                    day.to_string().into(),
                    label.into(),
                    input.entity_id.into(),
                    i64::from(is_active).into(),
                    now.into(),
                    id.into(),
                    input.expected_row_version.unwrap_or(current.row_version).into(),
                ],
            ))
            .await?;
        if res.rows_affected() == 0 {
            return Err(issue(
                "RECORD_CHANGED_ELSEWHERE",
                "This record was changed by someone else; reload and try again.",
            ));
        }
        id
    } else {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_calendar_holidays \
             (entity_sync_id, holiday_date, label, entity_id, is_active, row_version, created_by_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                day.to_string().into(),
                label.into(),
                input.entity_id.into(),
                i64::from(is_active).into(),
                actor_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
        txn.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("holiday: no row id")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?
    };
    let saved = get_holiday(&txn, id).await?;
    txn.commit().await?;
    Ok(saved)
}
