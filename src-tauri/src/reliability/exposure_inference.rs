use chrono::{DateTime, Datelike, Duration, NaiveDateTime, NaiveTime, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tracing::info;

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct ExposureComputation {
    pub hours: f64,
    pub source: String,
    pub schedule_reference_value_id: Option<i64>,
    pub utilization_factor: f64,
    pub last_completed_wo_closed_at: Option<String>,
}

#[derive(Debug, Clone)]
struct ScheduleShift {
    day_of_week: i64, // 1..7 (Mon..Sun)
    shift_start: NaiveTime,
    shift_end: NaiveTime,
    is_rest_day: bool,
}

fn parse_dt_utc(raw: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::ValidationFailed(vec![format!("invalid RFC3339 datetime '{raw}': {e}")]))
}

fn parse_time(raw: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(raw, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(raw, "%H:%M"))
        .map_err(|e| AppError::SyncError(format!("invalid schedule time '{raw}': {e}")))
}

fn overlap_hours(a0: DateTime<Utc>, a1: DateTime<Utc>, b0: DateTime<Utc>, b1: DateTime<Utc>) -> f64 {
    let lo = if a0 > b0 { a0 } else { b0 };
    let hi = if a1 < b1 { a1 } else { b1 };
    if hi <= lo {
        return 0.0;
    }
    (hi - lo).num_milliseconds() as f64 / 3_600_000.0
}

fn shift_hours_in_window(
    shift: &ScheduleShift,
    date_midnight_utc: DateTime<Utc>,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> f64 {
    if shift.is_rest_day {
        return 0.0;
    }
    let start_dt = Utc.from_utc_datetime(&NaiveDateTime::new(date_midnight_utc.date_naive(), shift.shift_start));
    let mut end_dt = Utc.from_utc_datetime(&NaiveDateTime::new(date_midnight_utc.date_naive(), shift.shift_end));
    if end_dt <= start_dt {
        end_dt += Duration::days(1);
    }
    overlap_hours(window_start, window_end, start_dt, end_dt)
}

fn compute_scheduled_hours(start: DateTime<Utc>, end: DateTime<Utc>, shifts: &[ScheduleShift]) -> f64 {
    if end <= start {
        return 0.0;
    }
    let mut total = 0.0_f64;
    let mut day = start.date_naive().and_hms_opt(0, 0, 0).expect("00:00 is always valid");
    let end_day = end.date_naive().and_hms_opt(0, 0, 0).expect("00:00 is always valid");

    while day <= end_day {
        let day_dt = Utc.from_utc_datetime(&day);
        let dow = i64::from(day.weekday().num_days_from_monday()) + 1;
        for shift in shifts.iter().filter(|s| s.day_of_week == dow) {
            total += shift_hours_in_window(shift, day_dt, start, end);
        }
        day += Duration::days(1);
    }
    total.max(0.0)
}

async fn runtime_exposure_sum_hours(
    db: &DatabaseConnection,
    equipment_id: i64,
    period_start: &str,
    period_end: &str,
    exclude_injected_sources: bool,
) -> AppResult<f64> {
    let exclude = if exclude_injected_sources { 1_i64 } else { 0_i64 };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(value), 0.0) AS texp FROM runtime_exposure_logs
             WHERE equipment_id = ? AND exposure_type = 'hours'
               AND recorded_at >= ? AND recorded_at <= ?
               AND (? = 0 OR COALESCE(source_type, '') <> 'rams_injector')",
            [
                equipment_id.into(),
                period_start.into(),
                period_end.into(),
                exclude.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("exposure sum missing.".into()))?;
    let t_exp: f64 = row
        .try_get("", "texp")
        .map_err(|e| AppError::SyncError(format!("decode texp failed: {e}")))?;
    Ok(t_exp.max(0.0))
}

pub async fn infer_exposure_hours(
    db: &DatabaseConnection,
    equipment_id: i64,
    period_start: &str,
    period_end: &str,
) -> AppResult<ExposureComputation> {
    fn log_decision(
        equipment_id: i64,
        source: &str,
        hours: f64,
        schedule_reference_value_id: Option<i64>,
        ku: f64,
        last_completed_wo_closed_at: Option<&str>,
    ) {
        info!(
            target: "maintafox",
            event = "rams.exposure_source_decision",
            equipment_id,
            source,
            exposure_hours = hours,
            schedule_reference_value_id = ?schedule_reference_value_id,
            utilization_factor = ku,
            last_completed_wo_closed_at = ?last_completed_wo_closed_at,
            "Exposure source selected for KPI/Weibull pipeline"
        );
    }

    let p0 = parse_dt_utc(period_start)?;
    let p1 = parse_dt_utc(period_end)?;
    let now = Utc::now();
    let window_end = if p1 < now { p1 } else { now };
    let exclude_injected_sources = crate::commands::product_license::is_product_activation_complete(db).await?;
    if exclude_injected_sources {
        info!(
            target: "maintafox",
            event = "rams.exposure_source_filter",
            equipment_id,
            source_type_excluded = "rams_injector",
            "Licensed tenant mode active: excluding injected runtime exposure rows"
        );
    }

    let eq_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rams_schedule_reference_value_id, COALESCE(rams_utilization_factor, 1.0) AS ku
             FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: equipment_id.to_string(),
        })?;

    let schedule_reference_value_id: Option<i64> = eq_row
        .try_get("", "rams_schedule_reference_value_id")
        .map_err(|e| AppError::SyncError(format!("decode rams_schedule_reference_value_id failed: {e}")))?;
    let ku: f64 = eq_row
        .try_get("", "ku")
        .map_err(|e| AppError::SyncError(format!("decode ku failed: {e}")))?;
    let ku = if ku.is_finite() && ku > 0.0 { ku } else { 1.0 };

    let now_s = now.to_rfc3339();
    let exclude_injector_wos = if exclude_injected_sources { 1_i64 } else { 0_i64 };
    let last_wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT closed_at FROM work_orders
             WHERE equipment_id = ? AND closed_at IS NOT NULL
               AND closed_at <= ?
               AND (? = 0 OR (
                     code NOT LIKE 'RAMS-INJECT%'
                     AND COALESCE(description, '') NOT LIKE '%[rams_injector]%'
                   ))
             ORDER BY closed_at DESC
             LIMIT 1",
            [equipment_id.into(), now_s.into(), exclude_injector_wos.into()],
        ))
        .await?;
    let last_closed_at: Option<String> = last_wo_row
        .map(|r| {
            r.try_get("", "closed_at")
                .map_err(|e| AppError::SyncError(format!("decode closed_at failed: {e}")))
        })
        .transpose()?;

    let fallback_hours =
        runtime_exposure_sum_hours(db, equipment_id, period_start, period_end, exclude_injected_sources).await?;

    let Some(last_closed_at_s) = last_closed_at.clone() else {
        let out = ExposureComputation {
            hours: fallback_hours,
            source: "runtime_exposure_logs_fallback_no_completed_wo".into(),
            schedule_reference_value_id,
            utilization_factor: ku,
            last_completed_wo_closed_at: None,
        };
        log_decision(
            equipment_id,
            &out.source,
            out.hours,
            out.schedule_reference_value_id,
            out.utilization_factor,
            None,
        );
        return Ok(out);
    };
    let last_closed_at_dt = parse_dt_utc(&last_closed_at_s)?;
    let window_start = if last_closed_at_dt > p0 { last_closed_at_dt } else { p0 };
    if window_end <= window_start {
        let out = ExposureComputation {
            hours: fallback_hours,
            source: "runtime_exposure_logs_fallback_non_positive_window".into(),
            schedule_reference_value_id,
            utilization_factor: ku,
            last_completed_wo_closed_at: Some(last_closed_at_s),
        };
        log_decision(
            equipment_id,
            &out.source,
            out.hours,
            out.schedule_reference_value_id,
            out.utilization_factor,
            out.last_completed_wo_closed_at.as_deref(),
        );
        return Ok(out);
    }
    let Some(sc_id) = schedule_reference_value_id else {
        let out = ExposureComputation {
            hours: fallback_hours,
            source: "runtime_exposure_logs_fallback_no_schedule".into(),
            schedule_reference_value_id,
            utilization_factor: ku,
            last_completed_wo_closed_at: Some(last_closed_at_s),
        };
        log_decision(
            equipment_id,
            &out.source,
            out.hours,
            out.schedule_reference_value_id,
            out.utilization_factor,
            out.last_completed_wo_closed_at.as_deref(),
        );
        return Ok(out);
    };

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT day_of_week, shift_start, shift_end, is_rest_day
             FROM schedule_details
             WHERE reference_value_id = ?
             ORDER BY day_of_week ASC",
            [sc_id.into()],
        ))
        .await?;
    if rows.is_empty() {
        let out = ExposureComputation {
            hours: fallback_hours,
            source: "runtime_exposure_logs_fallback_empty_schedule".into(),
            schedule_reference_value_id,
            utilization_factor: ku,
            last_completed_wo_closed_at: Some(last_closed_at_s),
        };
        log_decision(
            equipment_id,
            &out.source,
            out.hours,
            out.schedule_reference_value_id,
            out.utilization_factor,
            out.last_completed_wo_closed_at.as_deref(),
        );
        return Ok(out);
    }

    let mut shifts = Vec::with_capacity(rows.len());
    for row in rows {
        let day_of_week: i64 = row
            .try_get("", "day_of_week")
            .map_err(|e| AppError::SyncError(format!("decode day_of_week failed: {e}")))?;
        let shift_start_s: String = row
            .try_get("", "shift_start")
            .map_err(|e| AppError::SyncError(format!("decode shift_start failed: {e}")))?;
        let shift_end_s: String = row
            .try_get("", "shift_end")
            .map_err(|e| AppError::SyncError(format!("decode shift_end failed: {e}")))?;
        let is_rest_day_i: i64 = row
            .try_get("", "is_rest_day")
            .map_err(|e| AppError::SyncError(format!("decode is_rest_day failed: {e}")))?;
        shifts.push(ScheduleShift {
            day_of_week,
            shift_start: parse_time(&shift_start_s)?,
            shift_end: parse_time(&shift_end_s)?,
            is_rest_day: is_rest_day_i != 0,
        });
    }

    let scheduled_hours = compute_scheduled_hours(window_start, window_end, &shifts);
    let inferred = (scheduled_hours * ku).max(0.0);
    let out = ExposureComputation {
        hours: inferred,
        source: "calendar_operating_schedule_inferred".into(),
        schedule_reference_value_id,
        utilization_factor: ku,
        last_completed_wo_closed_at: Some(last_closed_at_s),
    };
    log_decision(
        equipment_id,
        &out.source,
        out.hours,
        out.schedule_reference_value_id,
        out.utilization_factor,
        out.last_completed_wo_closed_at.as_deref(),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .expect("valid RFC3339")
            .with_timezone(&Utc)
    }

    fn shift(day_of_week: i64, start: &str, end: &str, is_rest_day: bool) -> ScheduleShift {
        ScheduleShift {
            day_of_week,
            shift_start: parse_time(start).expect("valid shift start"),
            shift_end: parse_time(end).expect("valid shift end"),
            is_rest_day,
        }
    }

    #[test]
    fn computes_day_shift_hours_excluding_weekend() {
        let shifts = vec![
            shift(1, "08:00", "16:00", false),
            shift(2, "08:00", "16:00", false),
            shift(3, "08:00", "16:00", false),
            shift(4, "08:00", "16:00", false),
            shift(5, "08:00", "16:00", false),
            shift(6, "08:00", "16:00", true),
            shift(7, "08:00", "16:00", true),
        ];
        let hours = compute_scheduled_hours(
            dt("2026-04-06T00:00:00Z"), // Monday
            dt("2026-04-13T00:00:00Z"), // Next Monday
            &shifts,
        );
        assert!((hours - 40.0).abs() < 0.0001, "expected 40h, got {hours}");
    }

    #[test]
    fn computes_partial_overlap() {
        let shifts = vec![shift(1, "08:00", "16:00", false)];
        let hours = compute_scheduled_hours(dt("2026-04-06T10:00:00Z"), dt("2026-04-06T12:30:00Z"), &shifts);
        assert!((hours - 2.5).abs() < 0.0001, "expected 2.5h, got {hours}");
    }

    #[test]
    fn computes_overnight_shift() {
        let shifts = vec![shift(1, "22:00", "06:00", false)];
        let hours = compute_scheduled_hours(dt("2026-04-06T21:00:00Z"), dt("2026-04-07T04:00:00Z"), &shifts);
        assert!((hours - 6.0).abs() < 0.0001, "expected 6h, got {hours}");
    }

    /// Documents why future injector `closed_at` anchors must be excluded from SQL lookup.
    #[test]
    fn future_anchor_produces_non_positive_exposure_window() {
        let period_start = dt("2026-01-01T00:00:00Z");
        let window_end = dt("2026-06-08T12:00:00Z");
        let future_closed_at = dt("2026-07-25T00:03:16Z");
        let window_start = if future_closed_at > period_start {
            future_closed_at
        } else {
            period_start
        };
        assert!(
            window_end <= window_start,
            "future closed_at must not be used as exposure anchor"
        );
    }

    #[test]
    fn past_anchor_yields_positive_exposure_window() {
        let period_start = dt("2026-01-01T00:00:00Z");
        let window_end = dt("2026-06-08T12:00:00Z");
        let past_closed_at = dt("2026-04-28T21:03:46Z");
        let window_start = if past_closed_at > period_start {
            past_closed_at
        } else {
            period_start
        };
        assert!(window_end > window_start);
    }
}
