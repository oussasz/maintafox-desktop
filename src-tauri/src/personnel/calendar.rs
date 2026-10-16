//! Site calendar: holidays (`work_calendar_holidays`) — days nobody on the calendar works, named in
//! French and English, entered per year — and the weekly rest days (`site_work_weeks`), from a
//! date, for every site or for one organisation node and everything under it (Friday–Saturday by
//! default).

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
    pub label_en: Option<String>,
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
    #[serde(default)]
    pub label_en: Option<String>,
    pub entity_id: Option<i64>,
    pub is_active: Option<bool>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("holiday decode '{field}': {err}"))
}

fn issue(code: &str, message: &str) -> AppError {
    org_validation_failed(code, message, HashMap::new())
}

const SELECT: &str =
    "SELECT h.id, h.entity_sync_id, h.holiday_date, h.label, h.label_en, h.entity_id, n.name AS entity_name, \
        h.is_active, h.row_version \
     FROM work_calendar_holidays h LEFT JOIN org_nodes n ON n.id = h.entity_id";

fn map_row(r: &sea_orm::QueryResult) -> AppResult<WorkCalendarHoliday> {
    Ok(WorkCalendarHoliday {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        holiday_date: r
            .try_get("", "holiday_date")
            .map_err(|e| decode_err("holiday_date", e))?,
        label: r.try_get("", "label").map_err(|e| decode_err("label", e))?,
        label_en: r.try_get("", "label_en").ok().flatten(),
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
    let label_en = input
        .label_en
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
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
                "UPDATE work_calendar_holidays SET holiday_date = ?, label = ?, label_en = ?, entity_id = ?, is_active = ?, \
                 row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
                [
                    day.to_string().into(),
                    label.into(),
                    label_en.clone().into(),
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
             (entity_sync_id, holiday_date, label, label_en, entity_id, is_active, row_version, created_by_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                day.to_string().into(),
                label.into(),
                label_en.into(),
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

// ═══════════════════════════════════════════════════════════════════════════════
// Weekly rest days per site
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SiteWorkWeek {
    pub id: i64,
    pub entity_sync_id: String,
    pub entity_id: Option<i64>,
    pub entity_name: Option<String>,
    pub effective_from: String,
    /// ISO weekdays (1 = Monday … 7 = Sunday) nobody on the site works.
    pub weekend_days: Vec<i64>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiteWorkWeekInput {
    pub entity_id: Option<i64>,
    pub effective_from: String,
    pub weekend_days: Vec<i64>,
}

fn parse_weekend(raw: &str) -> Vec<i64> {
    let mut days: Vec<i64> = raw
        .split(',')
        .filter_map(|d| d.trim().parse::<i64>().ok())
        .filter(|d| (1..=7).contains(d))
        .collect();
    days.sort_unstable();
    days.dedup();
    days
}

const WEEK_SELECT: &str = "SELECT w.id, w.entity_sync_id, w.entity_id, n.name AS entity_name, w.effective_from, \
        w.weekend_days, w.row_version \
     FROM site_work_weeks w LEFT JOIN org_nodes n ON n.id = w.entity_id";

fn map_week(r: &sea_orm::QueryResult) -> AppResult<SiteWorkWeek> {
    Ok(SiteWorkWeek {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        entity_id: r.try_get("", "entity_id").ok().flatten(),
        entity_name: r.try_get("", "entity_name").ok().flatten(),
        effective_from: r
            .try_get("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        weekend_days: parse_weekend(
            &r.try_get::<String>("", "weekend_days")
                .map_err(|e| decode_err("weekend_days", e))?,
        ),
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

/// Every site's working-week history (all sites first, then by organisation node and date).
pub async fn list_site_work_weeks(db: &impl ConnectionTrait) -> AppResult<Vec<SiteWorkWeek>> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!("{WEEK_SELECT} ORDER BY w.entity_id IS NOT NULL, n.name, w.effective_from, w.id"),
    ))
    .await?
    .iter()
    .map(map_week)
    .collect()
}

/// Sets a site's (or every site's) weekly rest days from a date, today or later; a row already
/// starting that day is replaced, earlier rows stay as history.
pub async fn set_site_work_week(
    db: &DatabaseConnection,
    input: SiteWorkWeekInput,
    actor_id: i64,
) -> AppResult<Vec<SiteWorkWeek>> {
    let from = NaiveDate::parse_from_str(input.effective_from.trim(), "%Y-%m-%d")
        .map_err(|_| issue("TIMETABLE_DATE_INVALID", "Enter the date as YYYY-MM-DD."))?;
    let today = Utc::now().date_naive();
    if from < today {
        return Err(org_validation_failed(
            "TIMETABLE_DATE_IN_PAST",
            "A timetable change applies from today or a later date; past capacity is never rewritten.",
            HashMap::from([("today".to_string(), today.to_string())]),
        ));
    }
    if input.weekend_days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(issue(
            "SITE_WEEK_DAY_INVALID",
            "Weekdays run from Monday (1) to Sunday (7).",
        ));
    }
    let mut days = input.weekend_days.clone();
    days.sort_unstable();
    days.dedup();
    if days.len() == 7 {
        return Err(issue(
            "SITE_WEEK_NO_WORKING_DAY",
            "Leave at least one working day in the week.",
        ));
    }
    let text = days.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    if let Some(entity_id) = input.entity_id {
        if db
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
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let txn = db.begin().await?;
    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE site_work_weeks SET weekend_days = ?, row_version = row_version + 1, updated_at = ? \
             WHERE COALESCE(entity_id, 0) = COALESCE(?, 0) AND effective_from = ?",
            [
                text.clone().into(),
                now.clone().into(),
                input.entity_id.into(),
                from.to_string().into(),
            ],
        ))
        .await?;
    if updated.rows_affected() == 0 {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO site_work_weeks \
             (entity_sync_id, entity_id, effective_from, weekend_days, row_version, created_by_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                input.entity_id.into(),
                from.to_string().into(),
                text.into(),
                actor_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
    }
    txn.commit().await?;
    list_site_work_weeks(db).await
}

/// Weekly rest days that apply to an organisation node, by date: the node's own rows or its
/// nearest parent's, else every site's.
pub struct SiteWeekBook {
    /// Rows of the nearest node that has any (or every site's), oldest first.
    rows: Vec<(NaiveDate, Vec<i64>)>,
}

impl SiteWeekBook {
    pub async fn load(db: &impl ConnectionTrait, entity_id: Option<i64>) -> AppResult<Self> {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "WITH RECURSIVE ancestors(id, parent_id, depth) AS ( \
                    SELECT id, parent_id, 0 FROM org_nodes WHERE id = ? \
                    UNION ALL \
                    SELECT n.id, n.parent_id, a.depth + 1 FROM org_nodes n JOIN ancestors a ON n.id = a.parent_id \
                 ) \
                 SELECT w.effective_from, w.weekend_days, COALESCE(a.depth, 1000000) AS depth \
                 FROM site_work_weeks w LEFT JOIN ancestors a ON a.id = w.entity_id \
                 WHERE w.entity_id IS NULL OR a.id IS NOT NULL \
                 ORDER BY depth, w.effective_from",
                [entity_id.into()],
            ))
            .await?;
        let mut nearest: Option<i64> = None;
        let mut out = Vec::new();
        for r in rows {
            let depth: i64 = r.try_get("", "depth").map_err(|e| decode_err("depth", e))?;
            if nearest.is_some_and(|d| d != depth) {
                break;
            }
            nearest = Some(depth);
            let from: String = r
                .try_get("", "effective_from")
                .map_err(|e| decode_err("effective_from", e))?;
            if let Ok(from) = NaiveDate::parse_from_str(&from, "%Y-%m-%d") {
                out.push((
                    from,
                    parse_weekend(
                        &r.try_get::<String>("", "weekend_days")
                            .map_err(|e| decode_err("weekend_days", e))?,
                    ),
                ));
            }
        }
        Ok(Self { rows: out })
    }

    /// Whether `day` is a weekly rest day of the site.
    pub fn is_rest_day(&self, day: NaiveDate) -> bool {
        let weekday = i64::from(chrono::Datelike::weekday(&day).number_from_monday());
        self.rows
            .iter()
            .rev()
            .find(|(from, _)| *from <= day)
            .is_some_and(|(_, days)| days.contains(&weekday))
    }

    /// The site's weekend in force on `day` (ISO weekdays, 1 = Monday); `None` when no row
    /// applies yet.
    pub fn weekend_on(&self, day: NaiveDate) -> Option<&[i64]> {
        self.rows
            .iter()
            .rev()
            .find(|(from, _)| *from <= day)
            .map(|(_, days)| days.as_slice())
    }
}

/// Active holidays of a site between two days (inclusive): its own, its parents' and the ones of
/// every site.
pub async fn site_holidays(
    db: &impl ConnectionTrait,
    entity_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<NaiveDate>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE ancestors(id, parent_id) AS ( \
                SELECT id, parent_id FROM org_nodes WHERE id = ? \
                UNION ALL \
                SELECT n.id, n.parent_id FROM org_nodes n JOIN ancestors a ON n.id = a.parent_id \
             ) \
             SELECT DISTINCT holiday_date FROM work_calendar_holidays \
             WHERE is_active = 1 AND holiday_date BETWEEN ? AND ? \
               AND (entity_id IS NULL OR entity_id IN (SELECT id FROM ancestors)) \
             ORDER BY holiday_date",
            [entity_id.into(), from.to_string().into(), to.to_string().into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let raw: String = r
                .try_get("", "holiday_date")
                .map_err(|e| decode_err("holiday_date", e))?;
            NaiveDate::parse_from_str(raw.get(..10).unwrap_or(&raw), "%Y-%m-%d")
                .map_err(|e| decode_err("holiday_date", e))
        })
        .collect()
}
