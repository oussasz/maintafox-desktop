//! Tolerance windows and timing of PM occurrences (pure rules, no database).
//!
//! A version may carry a tolerance in its trigger definition:
//! `"tolerance": {"unit": "days" | "percent", "early": n, "late": n}`.
//!
//! - **Time window** (every occurrence that has a due date), whole UTC days:
//!   `[start of day(due − early), end of day(due + late)]`, where early/late are days, or a
//!   percentage of the calendar interval (`percent` on an occurrence without a calendar interval
//!   gives no early bound and ends with the due day). Without tolerance the window has no early
//!   bound and ends with the due day.
//! - **Meter window** (occurrences due at a meter threshold, `percent` only):
//!   `[threshold − early % × interval, threshold + late % × interval]` in meter units.
//! - **Timing** of the completed work: `early` before the window, `late` after it, `on_time`
//!   inside. A meter window, when the execution reading is known, decides; otherwise the time
//!   window does.

use chrono::{DateTime, Duration, Months, NaiveTime, Utc};
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToleranceUnit {
    Days,
    Percent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    pub unit: ToleranceUnit,
    pub early: f64,
    pub late: f64,
}

/// Reads `tolerance` from a trigger definition (absent → `None`).
pub fn parse_tolerance(trigger: &Map<String, Value>) -> AppResult<Option<Tolerance>> {
    let Some(raw) = trigger.get("tolerance") else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let Some(obj) = raw.as_object() else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PM_TOLERANCE_INVALID: tolerance must be an object {unit, early, late}.".to_string(),
        ]));
    };
    let unit = match obj.get("unit").and_then(Value::as_str).unwrap_or_default() {
        "days" => ToleranceUnit::Days,
        "percent" => ToleranceUnit::Percent,
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_PM_TOLERANCE_INVALID: tolerance.unit must be 'days' or 'percent' (got '{other}')."
            )]))
        }
    };
    let early = obj.get("early").and_then(Value::as_f64).unwrap_or(0.0);
    let late = obj.get("late").and_then(Value::as_f64).unwrap_or(0.0);
    if early < 0.0 || late < 0.0 || (unit == ToleranceUnit::Percent && (early > 100.0 || late > 100.0)) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PM_TOLERANCE_INVALID: tolerance early/late must be >= 0 (and <= 100 for a percentage).".to_string(),
        ]));
    }
    Ok(Some(Tolerance { unit, early, late }))
}

fn start_of_day(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.date_naive().and_time(NaiveTime::MIN).and_utc()
}

fn end_of_day(ts: DateTime<Utc>) -> DateTime<Utc> {
    start_of_day(ts) + Duration::days(1) - Duration::seconds(1)
}

fn fraction_of(interval: Duration, pct: f64) -> Duration {
    Duration::seconds((interval.num_seconds() as f64 * pct / 100.0).round() as i64)
}

/// Time window of an occurrence due at `due` (see module docs). `interval` is the calendar
/// interval of the version when it has one.
pub fn time_window(
    due: DateTime<Utc>,
    interval: Option<Duration>,
    tolerance: Option<&Tolerance>,
) -> (Option<DateTime<Utc>>, DateTime<Utc>) {
    match tolerance {
        Some(Tolerance {
            unit: ToleranceUnit::Days,
            early,
            late,
        }) => (
            Some(start_of_day(due - Duration::days(early.round() as i64))),
            end_of_day(due + Duration::days(late.round() as i64)),
        ),
        Some(Tolerance {
            unit: ToleranceUnit::Percent,
            early,
            late,
        }) => match interval {
            Some(interval) => (
                Some(start_of_day(due - fraction_of(interval, *early))),
                end_of_day(due + fraction_of(interval, *late)),
            ),
            None => (None, end_of_day(due)),
        },
        None => (None, end_of_day(due)),
    }
}

/// Meter window of an occurrence due at `threshold` (percent tolerance only).
pub fn meter_window(
    threshold: f64,
    interval: Option<f64>,
    tolerance: Option<&Tolerance>,
) -> (Option<f64>, Option<f64>) {
    match (tolerance, interval) {
        (
            Some(Tolerance {
                unit: ToleranceUnit::Percent,
                early,
                late,
            }),
            Some(interval),
        ) if interval > 0.0 => (
            Some(threshold - interval * early / 100.0),
            Some(threshold + interval * late / 100.0),
        ),
        _ => (None, None),
    }
}

/// Calendar recurrence of a fixed/floating version. Months and years follow the calendar (same
/// day of month, clamped to the month end: 31 Jan → 28/29 Feb → 31 Mar), never 30/365 days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recurrence {
    Every(Duration),
    Months(u32),
}

/// Reads `interval_unit` (hour, day, week, month, year) and `interval_value` (> 0).
pub fn parse_recurrence(trigger: &Map<String, Value>) -> AppResult<Recurrence> {
    let unit = trigger
        .get("interval_unit")
        .and_then(Value::as_str)
        .unwrap_or("day")
        .trim()
        .to_lowercase();
    let value = trigger.get("interval_value").and_then(Value::as_f64).unwrap_or(0.0);
    if value <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PM_INTERVAL_REQUIRED: interval_value must be > 0 for fixed/floating strategies.".to_string(),
        ]));
    }
    let amount = value.round().max(1.0) as i64;
    match unit.as_str() {
        "hour" | "hours" => Ok(Recurrence::Every(Duration::hours(amount))),
        "day" | "days" => Ok(Recurrence::Every(Duration::days(amount))),
        "week" | "weeks" => Ok(Recurrence::Every(Duration::weeks(amount))),
        "month" | "months" => Ok(Recurrence::Months(u32::try_from(amount).unwrap_or(u32::MAX))),
        "year" | "years" => Ok(Recurrence::Months(u32::try_from(amount * 12).unwrap_or(u32::MAX))),
        _ => Err(AppError::ValidationFailed(vec![
            "GATE_PM_INTERVAL_UNIT_INVALID: interval_unit must be one of: hour, day, week, month, year.".to_string(),
        ])),
    }
}

/// The `k`-th date of a recurrence counted from `anchor` (always from the anchor, so clamping
/// to a short month never shifts the following dates).
pub fn nth_date(anchor: DateTime<Utc>, recurrence: Recurrence, k: u32) -> DateTime<Utc> {
    match recurrence {
        Recurrence::Every(step) => anchor + step * i32::try_from(k).unwrap_or(i32::MAX),
        Recurrence::Months(months) => anchor
            .checked_add_months(Months::new(months.saturating_mul(k)))
            .unwrap_or(DateTime::<Utc>::MAX_UTC),
    }
}

/// Length of the recurrence period that starts at `at` (for percentage tolerances).
pub fn period_length(at: DateTime<Utc>, recurrence: Recurrence) -> Duration {
    nth_date(at, recurrence, 1) - at
}

/// Window bounds stored on an occurrence.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OccurrenceWindow {
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub start_meter: Option<f64>,
    pub end_meter: Option<f64>,
}

pub fn rfc3339(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `on_time` | `early` | `late` of work recorded at `executed_at` (meter `reading` when known).
pub fn classify_timing(
    window: &OccurrenceWindow,
    executed_at: DateTime<Utc>,
    reading: Option<f64>,
) -> Option<&'static str> {
    if let (Some(reading), true) = (reading, window.start_meter.is_some() || window.end_meter.is_some()) {
        if window.start_meter.is_some_and(|s| reading < s) {
            return Some("early");
        }
        if window.end_meter.is_some_and(|e| reading > e) {
            return Some("late");
        }
        return Some("on_time");
    }
    let end = window
        .end_at
        .as_deref()
        .and_then(|e| DateTime::parse_from_rfc3339(e).ok())?
        .with_timezone(&Utc);
    let start = window
        .start_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|s| s.with_timezone(&Utc));
    if start.is_some_and(|s| executed_at < s) {
        return Some("early");
    }
    if executed_at > end {
        return Some("late");
    }
    Some("on_time")
}

/// SMRP 5.4.1 PM compliance: share of the PM occurrences due in the period whose work was done
/// within their window. `None` when nothing was due.
pub fn compliance_pct(on_time: i64, due: i64) -> Option<f64> {
    if due <= 0 {
        None
    } else {
        Some((on_time as f64) * 100.0 / (due as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).expect("ts").with_timezone(&Utc)
    }

    #[test]
    fn monthly_recurrence_follows_the_calendar_without_drift() {
        let mut trigger = Map::new();
        trigger.insert("interval_unit".into(), Value::from("month"));
        trigger.insert("interval_value".into(), Value::from(1));
        let monthly = parse_recurrence(&trigger).expect("monthly");
        let anchor = ts("2026-10-15T00:00:00Z");
        let dates: Vec<String> = (0..4).map(|k| rfc3339(nth_date(anchor, monthly, k))).collect();
        assert_eq!(
            dates,
            vec![
                "2026-10-15T00:00:00Z",
                "2026-11-15T00:00:00Z",
                "2026-12-15T00:00:00Z",
                "2027-01-15T00:00:00Z"
            ]
        );
        // Clamped to the month end, then back to the anchor day.
        let end = ts("2027-01-31T00:00:00Z");
        assert_eq!(rfc3339(nth_date(end, monthly, 1)), "2027-02-28T00:00:00Z");
        assert_eq!(rfc3339(nth_date(end, monthly, 2)), "2027-03-31T00:00:00Z");
        trigger.insert("interval_unit".into(), Value::from("year"));
        let yearly = parse_recurrence(&trigger).expect("yearly");
        assert_eq!(
            rfc3339(nth_date(ts("2028-02-29T00:00:00Z"), yearly, 1)),
            "2029-02-28T00:00:00Z"
        );
    }

    #[test]
    fn days_tolerance_spans_whole_days() {
        let tol = Tolerance {
            unit: ToleranceUnit::Days,
            early: 2.0,
            late: 3.0,
        };
        let (start, end) = time_window(ts("2026-03-10T08:00:00Z"), None, Some(&tol));
        assert_eq!(rfc3339(start.expect("start")), "2026-03-08T00:00:00Z");
        assert_eq!(rfc3339(end), "2026-03-13T23:59:59Z");
    }

    #[test]
    fn percent_tolerance_is_a_share_of_the_interval() {
        let tol = Tolerance {
            unit: ToleranceUnit::Percent,
            early: 10.0,
            late: 10.0,
        };
        // 30-day interval → ±3 days.
        let (start, end) = time_window(ts("2026-03-10T00:00:00Z"), Some(Duration::days(30)), Some(&tol));
        assert_eq!(rfc3339(start.expect("start")), "2026-03-07T00:00:00Z");
        assert_eq!(rfc3339(end), "2026-03-13T23:59:59Z");
        assert_eq!(
            meter_window(1000.0, Some(250.0), Some(&tol)),
            (Some(975.0), Some(1025.0))
        );
    }

    #[test]
    fn no_tolerance_means_by_the_end_of_the_due_day() {
        let (start, end) = time_window(ts("2026-03-10T00:00:00Z"), Some(Duration::days(7)), None);
        assert!(start.is_none());
        assert_eq!(rfc3339(end), "2026-03-10T23:59:59Z");
    }

    #[test]
    fn timing_uses_the_meter_window_when_the_reading_is_known() {
        let window = OccurrenceWindow {
            start_at: Some("2026-03-07T00:00:00Z".into()),
            end_at: Some("2026-03-13T23:59:59Z".into()),
            start_meter: Some(975.0),
            end_meter: Some(1025.0),
        };
        assert_eq!(
            classify_timing(&window, ts("2026-03-20T00:00:00Z"), Some(1010.0)),
            Some("on_time")
        );
        assert_eq!(
            classify_timing(&window, ts("2026-03-10T00:00:00Z"), Some(1100.0)),
            Some("late")
        );
        assert_eq!(
            classify_timing(&window, ts("2026-03-06T10:00:00Z"), None),
            Some("early")
        );
        assert_eq!(classify_timing(&window, ts("2026-03-14T00:00:00Z"), None), Some("late"));
        assert_eq!(compliance_pct(3, 4), Some(75.0));
        assert_eq!(compliance_pct(0, 0), None);
    }
}
