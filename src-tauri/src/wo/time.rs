//! Shared timestamp helpers for WO lifecycle writes/reads.
//!
//! All WO mutation paths should use these helpers to keep UTC/Z formatting
//! consistent across intake, execution, closeout, and attachments.

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};

use crate::errors::{AppError, AppResult};

/// Canonical "now" in UTC RFC3339 with trailing `Z` and second precision.
pub fn now_utc_z() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Parse an input timestamp accepted by WO lifecycle APIs.
/// Supports RFC3339 and legacy `%Y-%m-%dT%H:%M:%SZ`.
pub fn parse_utc_timestamp(raw: &str, field: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%SZ")
                .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
        })
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 UTC timestamp.")]))
}

/// Stable codes: a planned start / planned end that is not a date and time.
pub const CODE_PLANNED_START_UNREADABLE: &str = "WO_PLANNED_START_UNREADABLE";
pub const CODE_PLANNED_END_UNREADABLE: &str = "WO_PLANNED_END_UNREADABLE";

/// A planned date and time as an instant, read like planning reads it
/// (`wo::shifts::planned_instant`): RFC 3339 as stored; typed without a zone, in this computer's
/// time zone. A bare day or any other text is not a time of day: `None`.
pub fn planned_time(raw: &str) -> Option<DateTime<Utc>> {
    match super::shifts::planned_local(Some(raw)) {
        Some((_, Some(_))) => super::shifts::planned_instant(raw),
        _ => None,
    }
}

/// A planned date and time (`planned_start` with [`CODE_PLANNED_START_UNREADABLE`], `planned_end`
/// with [`CODE_PLANNED_END_UNREADABLE`]) in its stored form: UTC RFC 3339 with `Z`, whole seconds,
/// so every reader (the permit start gate, planning, another PC) reads the same instant. Accepts an
/// RFC 3339 instant, or a date and time typed without a zone (a `datetime-local` field), read in
/// this computer's time zone as planning reads it (`wo::shifts::planned_instant`), never as UTC.
/// Blank: `None` (no date). A bare day or any other text is refused (coded, FR/EN on screen).
pub fn stored_planned_time(raw: Option<&str>, code: &str) -> AppResult<Option<String>> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    match planned_time(raw) {
        Some(at) => Ok(Some(at.to_rfc3339_opts(SecondsFormat::Secs, true))),
        None => Err(crate::errors::org_validation_failed(
            code,
            format!("The planned date ({raw}) is not a readable date and time: re-plan the work order's dates."),
            std::collections::HashMap::from([("value".to_string(), raw.to_string())]),
        )),
    }
}
