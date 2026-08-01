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
