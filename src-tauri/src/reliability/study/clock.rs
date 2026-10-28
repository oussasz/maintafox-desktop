//! Time base of a study: the clock that turns two instants into hours of use.
//!
//! Per equipment, in order of evidence:
//! 1. a run-hours meter (`equipment_meters.meter_type = HOURS`) with at least two readings —
//!    hours are the meter increase, interpolated linearly between readings; a meter that goes
//!    down (replaced or reset) adds nothing for that segment;
//! 2. the governed exposure basis of the RAMS module (`exposure_inference::load_exposure_basis`:
//!    runtime exposure logs, or the operating schedule × utilisation factor);
//! 3. calendar hours, shown as such on screen.
//!
//! A study uses operating hours only when every one of its equipment has an operating clock,
//! so pooled ages always share one unit.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reliability::exposure_inference::{load_exposure_basis, ExposureBasis};

pub const SOURCE_METER: &str = "run_hours_meter";
pub const SOURCE_RUNTIME_LOGS: &str = "runtime_logs";
pub const SOURCE_SCHEDULE: &str = "operating_schedule";
pub const SOURCE_CALENDAR: &str = "calendar";

#[derive(Debug, Clone)]
pub enum StudyClock {
    /// Cumulative run-hours meter readings `(instant, reading)`, sorted by instant.
    Meter(Vec<(DateTime<Utc>, f64)>),
    Exposure(ExposureBasis),
    Calendar,
}

fn calendar_hours(t0: DateTime<Utc>, t1: DateTime<Utc>) -> f64 {
    if t1 <= t0 {
        0.0
    } else {
        (t1 - t0).num_milliseconds() as f64 / 3_600_000.0
    }
}

impl StudyClock {
    #[must_use]
    pub fn source(&self) -> &'static str {
        match self {
            Self::Meter(_) => SOURCE_METER,
            Self::Exposure(ExposureBasis::RuntimeLogs { .. }) => SOURCE_RUNTIME_LOGS,
            Self::Exposure(ExposureBasis::Schedule { .. }) => SOURCE_SCHEDULE,
            Self::Calendar => SOURCE_CALENDAR,
        }
    }

    #[must_use]
    pub fn is_operating(&self) -> bool {
        !matches!(self, Self::Calendar)
    }

    /// Hours of use in `(t0, t1]`.
    #[must_use]
    pub fn hours_between(&self, t0: DateTime<Utc>, t1: DateTime<Utc>) -> f64 {
        if t1 <= t0 {
            return 0.0;
        }
        match self {
            Self::Calendar => calendar_hours(t0, t1),
            Self::Exposure(basis) => basis.hours_between(t0, t1),
            Self::Meter(readings) => {
                let mut total = 0.0;
                for w in readings.windows(2) {
                    let (a_t, a_v) = w[0];
                    let (b_t, b_v) = w[1];
                    let span = calendar_hours(a_t, b_t);
                    if span <= 0.0 {
                        continue;
                    }
                    let lo = if t0 > a_t { t0 } else { a_t };
                    let hi = if t1 < b_t { t1 } else { b_t };
                    let overlap = calendar_hours(lo, hi);
                    if overlap > 0.0 {
                        total += (b_v - a_v).max(0.0) * overlap / span;
                    }
                }
                total
            }
        }
    }

    /// Share of `[start, end]` covered by the clock's data (1 for a schedule or calendar).
    #[must_use]
    pub fn coverage(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> f64 {
        let period = calendar_hours(start, end);
        if period <= 0.0 {
            return 0.0;
        }
        let span = |first: DateTime<Utc>, last: DateTime<Utc>| {
            let lo = if start > first { start } else { first };
            let hi = if end < last { end } else { last };
            (calendar_hours(lo, hi) / period).clamp(0.0, 1.0)
        };
        match self {
            Self::Calendar | Self::Exposure(ExposureBasis::Schedule { .. }) => 1.0,
            Self::Meter(readings) => match (readings.first(), readings.last()) {
                (Some((f, _)), Some((l, _))) => span(*f, *l),
                _ => 0.0,
            },
            Self::Exposure(ExposureBasis::RuntimeLogs { increments }) => {
                match (increments.first(), increments.last()) {
                    (Some((f, _)), Some((l, _))) => span(*f, *l),
                    _ => 0.0,
                }
            }
        }
    }
}

fn parse_ts(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw.trim(), "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        })
}

/// Readings of the equipment's run-hours meter (primary first), when it has at least two.
async fn load_meter_readings(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<Option<Vec<(DateTime<Utc>, f64)>>> {
    let meter = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT m.id FROM equipment_meters m
             WHERE m.equipment_id = ? AND m.is_active = 1 AND upper(m.meter_type) = 'HOURS'
               AND (SELECT COUNT(*) FROM asset_meter_readings r
                    WHERE r.meter_id = m.id AND COALESCE(r.quality_flag, 'accepted') <> 'rejected') >= 2
             ORDER BY m.is_primary DESC, m.id ASC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;
    let Some(meter) = meter else {
        return Ok(None);
    };
    let meter_id: i64 = meter
        .try_get("", "id")
        .map_err(|e| AppError::SyncError(format!("study clock decode meter id: {e}")))?;
    // Readings after a meter replacement carry the lifetime value (counter offset) when the
    // schema has it; the raw reading otherwise.
    let has_lifetime = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info('asset_meter_readings') WHERE name = 'lifetime_value'".to_string(),
        ))
        .await?
        .is_some();
    let value_expr = if has_lifetime {
        "COALESCE(lifetime_value, reading_value)"
    } else {
        "reading_value"
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT reading_at, {value_expr} AS reading_value FROM asset_meter_readings
                 WHERE meter_id = ? AND COALESCE(quality_flag, 'accepted') <> 'rejected'
                 ORDER BY reading_at ASC, id ASC"
            ),
            [meter_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let at: String = r
            .try_get("", "reading_at")
            .map_err(|e| AppError::SyncError(format!("study clock decode reading_at: {e}")))?;
        let v: f64 = r
            .try_get("", "reading_value")
            .map_err(|e| AppError::SyncError(format!("study clock decode reading_value: {e}")))?;
        if let Some(t) = parse_ts(&at) {
            if v.is_finite() {
                out.push((t, v));
            }
        }
    }
    Ok((out.len() >= 2).then_some(out))
}

/// Best clock for one equipment (meter, governed exposure basis, calendar).
pub async fn load_clock(db: &DatabaseConnection, equipment_id: i64) -> AppResult<StudyClock> {
    if let Some(readings) = load_meter_readings(db, equipment_id).await? {
        return Ok(StudyClock::Meter(readings));
    }
    if let Some(basis) = load_exposure_basis(db, equipment_id).await? {
        return Ok(StudyClock::Exposure(basis));
    }
    Ok(StudyClock::Calendar)
}

/// Parse an RFC 3339 (or SQLite `YYYY-MM-DD HH:MM:SS`) timestamp.
pub fn parse_instant(raw: &str) -> AppResult<DateTime<Utc>> {
    parse_ts(raw).ok_or_else(|| AppError::ValidationFailed(vec![format!("GATE_REL_DATE_INVALID: invalid date: {raw}")]))
}

/// Lenient variant for stored values.
#[must_use]
pub fn parse_instant_opt(raw: Option<&str>) -> Option<DateTime<Utc>> {
    raw.and_then(parse_ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn meter_hours_are_interpolated_and_resets_add_nothing() {
        // 0 h on Jan 1 00:00, 24 h on Jan 2 00:00 (ran all day), 24 h on Jan 3 (stopped),
        // replaced meter reads 5 on Jan 4 (reset → that segment adds nothing), 17 on Jan 5.
        let clock = StudyClock::Meter(vec![
            (at(1, 0), 0.0),
            (at(2, 0), 24.0),
            (at(3, 0), 24.0),
            (at(4, 0), 5.0),
            (at(5, 0), 17.0),
        ]);
        assert!((clock.hours_between(at(1, 0), at(2, 0)) - 24.0).abs() < 1e-9);
        assert!((clock.hours_between(at(1, 12), at(2, 0)) - 12.0).abs() < 1e-9);
        assert!((clock.hours_between(at(2, 0), at(4, 0)) - 0.0).abs() < 1e-9);
        assert!((clock.hours_between(at(4, 0), at(5, 0)) - 12.0).abs() < 1e-9);
        assert!((clock.hours_between(at(1, 0), at(5, 0)) - 36.0).abs() < 1e-9);
        // Outside the readings nothing is known.
        assert_eq!(clock.hours_between(at(5, 0), at(6, 0)), 0.0);
        assert!((clock.coverage(at(1, 0), at(9, 0)) - 0.5).abs() < 1e-9);
        assert_eq!(clock.source(), SOURCE_METER);
    }

    #[test]
    fn calendar_clock_counts_wall_time() {
        let clock = StudyClock::Calendar;
        assert!((clock.hours_between(at(1, 0), at(2, 6)) - 30.0).abs() < 1e-9);
        assert!(!clock.is_operating());
        assert_eq!(clock.coverage(at(1, 0), at(2, 0)), 1.0);
    }
}
