//! Fiscal calendar: the month the company's fiscal year starts in, a tenant setting kept with the
//! currency in Settings & Preferences (`locale.fiscal_year_start_month`, seeded to January).
//!
//! Budget periods are numbered 1..12 from the start month: with a start in July, period 1 is
//! July and period 12 is June. A fiscal year is named by the calendar year it starts in. With the
//! default start (January) periods are calendar months and fiscal years calendar years.

use chrono::{Datelike, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

pub const FISCAL_START_SETTING: &str = "locale.fiscal_year_start_month";

/// Parses the stored setting value (a JSON number, or a numeric string from the editor).
pub fn parse_start_month(raw: &str) -> Option<i64> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let month = match value {
        serde_json::Value::Number(n) => n.as_i64()?,
        serde_json::Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (1..=12).contains(&month).then_some(month)
}

/// The tenant's fiscal-year start month. A missing or invalid setting is an error (never a
/// guessed calendar): the seeder writes January on every startup when the row is absent.
pub async fn fiscal_start_month<C: ConnectionTrait>(db: &C) -> AppResult<i64> {
    let raw: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = ? LIMIT 1",
            [FISCAL_START_SETTING.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "setting_value_json").ok());
    raw.as_deref().and_then(parse_start_month).ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "Le mois de début de l'exercice n'est pas configuré (Paramètres › Localisation). \
             (The fiscal-year start month is not configured: Settings › Localization.)"
                .to_string(),
        ])
    })
}

/// `(fiscal year, period 1..12)` of a calendar `(year, month)`.
pub fn fiscal_period(year: i64, month: i64, start: i64) -> (i64, i64) {
    let fiscal_year = if month >= start { year } else { year - 1 };
    (fiscal_year, (month - start).rem_euclid(12) + 1)
}

/// Calendar `(year, month)` of a fiscal period.
pub fn calendar_month(fiscal_year: i64, period: i64, start: i64) -> (i64, i64) {
    let month0 = start - 1 + (period - 1);
    (fiscal_year + month0 / 12, month0 % 12 + 1)
}

/// Fiscal `(year, period)` of today.
pub fn current_period(start: i64) -> (i64, i64) {
    let now = Utc::now();
    fiscal_period(i64::from(now.year()), i64::from(now.month()), start)
}

/// SQL expression: fiscal year of an ISO timestamp column, start month bound at `start_param`.
pub fn sql_fiscal_year(column: &str, start_param: &str) -> String {
    format!(
        "(CAST(strftime('%Y', {column}) AS INTEGER) - \
          CASE WHEN CAST(strftime('%m', {column}) AS INTEGER) < {start_param} THEN 1 ELSE 0 END)"
    )
}

/// SQL expression: fiscal period (1..12) of an ISO timestamp column.
pub fn sql_fiscal_period(column: &str, start_param: &str) -> String {
    format!("(((CAST(strftime('%m', {column}) AS INTEGER) - {start_param} + 12) % 12) + 1)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn january_start_is_the_calendar() {
        assert_eq!(fiscal_period(2026, 1, 1), (2026, 1));
        assert_eq!(fiscal_period(2026, 12, 1), (2026, 12));
        assert_eq!(calendar_month(2026, 7, 1), (2026, 7));
    }

    #[test]
    fn july_start_shifts_periods_and_years() {
        assert_eq!(fiscal_period(2026, 7, 7), (2026, 1));
        assert_eq!(fiscal_period(2027, 6, 7), (2026, 12));
        assert_eq!(fiscal_period(2026, 1, 7), (2025, 7));
        assert_eq!(calendar_month(2026, 1, 7), (2026, 7));
        assert_eq!(calendar_month(2026, 12, 7), (2027, 6));
        for month in 1..=12 {
            let (fy, p) = fiscal_period(2026, month, 7);
            assert_eq!(calendar_month(fy, p, 7), (2026, month));
        }
    }

    #[test]
    fn setting_values_are_validated() {
        assert_eq!(parse_start_month("1"), Some(1));
        assert_eq!(parse_start_month("\"7\""), Some(7));
        assert_eq!(parse_start_month("13"), None);
        assert_eq!(parse_start_month("\"x\""), None);
    }
}
