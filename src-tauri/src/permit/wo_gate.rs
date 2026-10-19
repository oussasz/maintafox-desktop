//! Permit gate of work order execution: an OT starts, resumes or is reopened to in-progress only
//! when every live permit linked to it is active and ready to protect the work (not expired, LOTO
//! isolations all applied and verified, none removed) — whether or not the OT is flagged as
//! requiring a permit — and, when it requires one, at least one exists; when a plant rule flagged it
//! at conversion (and the flag was not cleared), at least one is of the rule's permit type. A live
//! permit that expires before the end of the work refuses the start too (owner decision: the work
//! would outlive its protection). The end of the work (CMMS Advisor decision, see [`work_end`]) is
//! the later of the planned end and the moment of the start, resume or reopening plus the expected
//! duration — or, on a resume or reopening, the estimated remaining time the user entered instead.
//! A planned end already past with no duration to add is refused ("re-plan the dates or enter the
//! duration"); without a planned end or a duration nothing is judged (the "permit expiring"
//! notification still gives the expiry date and time). Work already in progress is only warned
//! (planning and the expiry notification). Planning only warns about permits; this gate is where
//! the rule is hard-enforced. Callers run it inside the transaction that changes the status.

use std::collections::HashMap;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};

use super::queries::{activation_ready, list_live_permits_linked_to_work_order};

/// Gate of a start judged now: the end of the work uses the full expected duration.
pub async fn assert_in_progress_permit_gate(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    assert_in_progress_permit_gate_with(db, wo_id, None, crate::wo::time::now_utc_second()).await
}

/// Gate of a start, resume or reopening at `at` — the moment the caller records as the start,
/// resume or reopening (one clock read, `wo::time::now_utc_second`), so the arrival check on another
/// PC judges the identical moment. `remaining_hours`: the estimated remaining time entered on a
/// resume or reopening ([`checked_remaining_hours`]), used instead of the expected duration for this
/// check only; `None`: the full expected duration.
pub async fn assert_in_progress_permit_gate_with(
    db: &impl ConnectionTrait,
    wo_id: i64,
    remaining_hours: Option<f64>,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_permit, planned_end, \
             CASE WHEN typeof(expected_duration_hours) IN ('real', 'integer') THEN expected_duration_hours END AS hours_number, \
             CASE typeof(expected_duration_hours) WHEN 'text' THEN expected_duration_hours \
                  WHEN 'blob' THEN 'blob' END AS hours_text, \
             CASE WHEN permit_rule_cleared_at IS NULL THEN permit_rule_permit_type END AS rule_permit_type \
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let requires: i64 = row
        .try_get("", "requires_permit")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("requires_permit decode: {e}")))?;
    let rule_permit_type: Option<String> = row
        .try_get("", "rule_permit_type")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("rule_permit_type decode: {e}")))?;
    let planned_end: Option<String> = row
        .try_get("", "planned_end")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("planned_end decode: {e}")))?;
    let hours_number: Option<f64> = row
        .try_get("", "hours_number")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("expected_duration_hours decode: {e}")))?;
    let hours_text: Option<String> = row
        .try_get("", "hours_text")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("expected_duration_hours decode: {e}")))?;
    // Stored as text or a blob (a forged or corrupt row): read as the arrival check reads a value
    // that is not a number (a blob is never one).
    let expected = match hours_number {
        Some(h) => Hours::Value(h),
        None => Hours::from_text(hours_text.as_deref()),
    };
    let required = requires == 1;
    // The work starts or resumes at `at`: the end it must be covered until.
    let end = work_end(planned_end.as_deref(), &expected, remaining_hours, at);
    assert_live_permits_ready(
        db,
        wo_id,
        required,
        rule_permit_type.filter(|_| required).as_deref(),
        &end,
    )
    .await
}

/// Stable code: a live permit of the work order expires before its planned end.
pub const CODE_EXPIRES_BEFORE_PLANNED_END: &str = "PERMIT_EXPIRES_BEFORE_PLANNED_END";
/// Stable code: a live permit expires before the start or resume moment plus the work order's
/// expected duration (or the estimated remaining time entered), an end later than any planned end.
pub const CODE_EXPIRES_BEFORE_ESTIMATED_END: &str = "PERMIT_EXPIRES_BEFORE_ESTIMATED_END";
/// Stable code: the work order's planned end is not a readable date and time (re-plan the dates).
pub const CODE_PLANNED_END_UNREADABLE: &str = crate::wo::time::CODE_PLANNED_END_UNREADABLE;
/// Stable code: the planned end is already past and there is no duration to add to the start or
/// resume moment (re-plan the dates, or enter the expected duration or the remaining time).
pub const CODE_PLANNED_END_PASSED: &str = "WO_PLANNED_END_PASSED";
/// Stable code: the work order's expected duration is stored as something that is not a number.
pub const CODE_EXPECTED_DURATION_UNREADABLE: &str = "WO_EXPECTED_DURATION_UNREADABLE";
/// Stable code: an estimated remaining time that is not a number of hours greater than 0.
pub const CODE_REMAINING_TIME_INVALID: &str = "WO_REMAINING_TIME_INVALID";

/// A number of hours as stored or received: absent (NULL, missing, blank), a number, or something
/// that is not a number. Read the same way on this PC and on arrival.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Hours {
    Absent,
    Value(f64),
    Unreadable(String),
}

impl Hours {
    /// A number of hours given as text: blank is absent; a number (Rust's reading; SQLite's REAL
    /// affinity stores numeric text as that number) is that number; anything else is unreadable.
    pub(crate) fn from_text(raw: Option<&str>) -> Hours {
        match raw.map(str::trim).filter(|t| !t.is_empty()) {
            None => Hours::Absent,
            Some(t) => t
                .parse::<f64>()
                .map_or_else(|_| Hours::Unreadable(t.to_string()), Hours::Value),
        }
    }

    /// The hours to add to the start or resume moment: a finite number greater than 0.
    pub(crate) fn usable(&self) -> Option<f64> {
        match self {
            Hours::Value(h) if h.is_finite() && *h > 0.0 => Some(*h),
            _ => None,
        }
    }
}

/// An estimated remaining time entered on a resume or reopening: a finite number of hours greater
/// than 0, or none (the full expected duration is used). Never stored over the planned or expected
/// duration; recorded on the transition-log row of that resume or reopening.
pub fn checked_remaining_hours(remaining_hours: Option<f64>) -> AppResult<Option<f64>> {
    match remaining_hours {
        None => Ok(None),
        Some(h) if h.is_finite() && h > 0.0 => Ok(Some(h)),
        Some(h) => Err(remaining_time_invalid(&h.to_string())),
    }
}

/// The refusal of an estimated remaining time that is not a number of hours greater than 0.
pub(crate) fn remaining_time_invalid(value: &str) -> AppError {
    org_validation_failed(
        CODE_REMAINING_TIME_INVALID,
        format!("The estimated remaining time ({value}) must be a number of hours greater than 0."),
        HashMap::from([("value".to_string(), value.to_string())]),
    )
}

/// The end of the work a live permit must cover, as of the moment the work starts or resumes
/// (owner decision; CMMS Advisor, 2026-10-19, and its follow-up on late work).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WorkEnd {
    /// The work order's planned end (not earlier than the moment plus any duration).
    Planned(DateTime<Utc>),
    /// The moment of the start or resume plus `hours` (the expected duration, or the estimated
    /// remaining time when `remaining`), later than any planned end.
    Estimated {
        end: DateTime<Utc>,
        hours: f64,
        remaining: bool,
    },
    /// Neither a planned end nor a duration: not judged.
    Unknown,
    /// A planned end that is not a readable date and time: refused (re-plan the dates), never guessed.
    Unreadable(String),
    /// A planned end already past (at or before the moment) with no duration to add: the end of the
    /// work is unknown; refused (re-plan the dates, or enter the duration or the remaining time).
    PlannedPassed(DateTime<Utc>),
    /// An expected duration stored as something that is not a number (and no remaining time
    /// entered): refused, never guessed.
    DurationUnreadable(String),
}

/// The end of the work started or resumed at `at`: the later of the planned end and `at` plus the
/// hours of work — the estimated remaining time when one was entered (resume, reopening), otherwise
/// the expected duration when it is a positive number. A planned end already past with no hours
/// to add is [`WorkEnd::PlannedPassed`]; neither a planned end nor hours: unknown (not judged).
pub(crate) fn work_end(
    planned_end: Option<&str>,
    expected: &Hours,
    remaining_hours: Option<f64>,
    at: DateTime<Utc>,
) -> WorkEnd {
    let planned = match planned_end.map(str::trim).filter(|e| !e.is_empty()) {
        Some(raw) => match crate::wo::time::planned_time(raw, crate::wo::time::PlannedBound::End) {
            Some(p) => Some(p),
            None => return WorkEnd::Unreadable(raw.to_string()),
        },
        None => None,
    };
    let hours = match remaining_hours.filter(|h| h.is_finite() && *h > 0.0) {
        Some(h) => Some((h, true)),
        None => match expected {
            Hours::Unreadable(raw) => return WorkEnd::DurationUnreadable(raw.clone()),
            other => other.usable().map(|h| (h, false)),
        },
    };
    match (planned, hours) {
        (planned, Some((hours, remaining))) => {
            // Whole seconds rounded up, so a tiny time never ends before the moment plus that time
            // (never earlier); a duration beyond the calendar ends after any expiry.
            let end = Duration::try_seconds((hours * 3600.0).ceil() as i64)
                .and_then(|d| at.checked_add_signed(d))
                .unwrap_or(DateTime::<Utc>::MAX_UTC);
            match planned {
                Some(p) if p >= end => WorkEnd::Planned(p),
                _ => WorkEnd::Estimated { end, hours, remaining },
            }
        }
        (Some(p), None) if p <= at => WorkEnd::PlannedPassed(p),
        (Some(p), None) => WorkEnd::Planned(p),
        (None, None) => WorkEnd::Unknown,
    }
}

/// The refusal code when a permit expiring at `expiry` does not cover the work until `end`:
/// strictly earlier than the end (an expiry equal to it covers the work). An end that cannot be
/// known (unreadable, planned end past without a duration, unreadable duration) refuses any permit
/// with an expiry.
pub(crate) fn expiry_refusal(expiry: DateTime<Utc>, end: &WorkEnd) -> Option<&'static str> {
    match end {
        WorkEnd::Planned(e) if expiry < *e => Some(CODE_EXPIRES_BEFORE_PLANNED_END),
        WorkEnd::Estimated { end: e, .. } if expiry < *e => Some(CODE_EXPIRES_BEFORE_ESTIMATED_END),
        WorkEnd::Unreadable(_) => Some(CODE_PLANNED_END_UNREADABLE),
        WorkEnd::PlannedPassed(_) => Some(CODE_PLANNED_END_PASSED),
        WorkEnd::DurationUnreadable(_) => Some(CODE_EXPECTED_DURATION_UNREADABLE),
        _ => None,
    }
}

/// An instant as stored (UTC, `Z`, whole seconds): the form the screens show in local time.
pub(crate) fn stored(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Hours as sent in a refusal: rounded to at most two decimals, no trailing zeros, `.` as the
/// decimal separator (the screens show it in the user's language: `2,5` in French).
pub(crate) fn shown_hours(hours: f64) -> String {
    let rounded = (hours * 100.0).round() / 100.0;
    if rounded.is_finite() {
        let text = format!("{rounded:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        hours.to_string()
    }
}

/// The refusal of a work order whose planned end cannot be read.
pub(crate) fn planned_end_unreadable(raw: &str) -> AppError {
    org_validation_failed(
        CODE_PLANNED_END_UNREADABLE,
        format!(
            "The work order's planned end ({raw}) is not a readable date and time: re-plan the work order's dates."
        ),
        HashMap::from([("value".to_string(), raw.to_string())]),
    )
}

/// The refusal of a work order whose expected duration is stored as something that is not a number.
pub(crate) fn duration_unreadable(raw: &str) -> AppError {
    org_validation_failed(
        CODE_EXPECTED_DURATION_UNREADABLE,
        format!(
            "The work order's expected duration ({raw}) is not a number of hours: correct it before starting or resuming the work."
        ),
        HashMap::from([("value".to_string(), raw.to_string())]),
    )
}

/// The permit covers the work until its end (see [`work_end`]; the permit's expiry is read as an
/// instant, an unreadable one is refused rather than guessed).
fn covers_the_work(permit_code: &str, expires_at: Option<&str>, end: &WorkEnd) -> AppResult<()> {
    let Some(expiry_raw) = expires_at.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    match end {
        WorkEnd::Unreadable(raw) => return Err(planned_end_unreadable(raw)),
        WorkEnd::DurationUnreadable(raw) => return Err(duration_unreadable(raw)),
        _ => {}
    }
    let Some(expiry) = crate::settings::site::instant(expiry_raw) else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Permit {permit_code}: its expiry ({expiry_raw}) is not a readable time."
        )]));
    };
    let mut params = HashMap::from([
        ("permit".to_string(), permit_code.to_string()),
        ("expires_at".to_string(), expiry_raw.to_string()),
    ]);
    match (expiry_refusal(expiry, end), end) {
        (Some(code), WorkEnd::Planned(e)) => {
            let e = stored(*e);
            let message = format!(
                "Permit {permit_code} expires ({expiry_raw}) before the work order's planned end ({e}): issue a permit that covers the planned work, or re-plan the work, before starting or resuming it."
            );
            params.insert("planned_end".to_string(), e);
            Err(org_validation_failed(code, message, params))
        }
        (
            Some(code),
            WorkEnd::Estimated {
                end: e,
                hours,
                remaining,
            },
        ) => {
            let e = stored(*e);
            let hours = shown_hours(*hours);
            let basis = if *remaining {
                "the estimated remaining time entered"
            } else {
                "the expected duration"
            };
            let message = format!(
                "Permit {permit_code} expires ({expiry_raw}) before the estimated end of the work ({e}: now + {hours} h, {basis}; later than any planned end): issue a permit that covers the work, or extend this permit if it is not active yet, before starting or resuming it."
            );
            params.insert("end_at".to_string(), e);
            params.insert("hours".to_string(), hours);
            Err(org_validation_failed(code, message, params))
        }
        (Some(code), WorkEnd::PlannedPassed(p)) => {
            let p = stored(*p);
            let message = format!(
                "The work order's planned end ({p}) has passed and it has no usable expected duration (none, or 0 h or less), so the end of the work covered by permit {permit_code} (expires {expiry_raw}) is unknown: re-plan the work order's dates, or enter its expected duration (or, when resuming or reopening, the estimated remaining time)."
            );
            params.insert("planned_end".to_string(), p);
            Err(org_validation_failed(code, message, params))
        }
        _ => Ok(()),
    }
}

/// Every live permit linked to the work order is active and ready (a permit that exists protects
/// the work, flag or not); when the OT requires a permit, at least one exists, and when a plant rule
/// names the permit type, at least one is of that type (a permit of another type does not protect
/// the hazard the rule is about); no live permit expires before the end of the work.
async fn assert_live_permits_ready(
    db: &impl ConnectionTrait,
    wo_id: i64,
    required: bool,
    rule_permit_type: Option<&str>,
    end: &WorkEnd,
) -> AppResult<()> {
    let permits = list_live_permits_linked_to_work_order(db, wo_id).await?;
    if permits.is_empty() && required {
        return Err(org_validation_failed(
            "PERMIT_REQUIRED_TO_START",
            "This work order requires a work permit: link an active permit before starting or resuming it.",
            HashMap::new(),
        ));
    }
    if let Some(permit_type) = rule_permit_type {
        if !permits
            .iter()
            .any(|p| p.permit_type_code.as_deref() == Some(permit_type))
        {
            let name = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT name FROM permit_types WHERE code = ?",
                    [permit_type.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<String>("", "name").ok())
                .unwrap_or_else(|| permit_type.to_string());
            return Err(org_validation_failed(
                "PERMIT_TYPE_REQUIRED_TO_START",
                format!(
                    "A permit rule requires a {name} permit for this work order: link an active permit of that type before starting or resuming it."
                ),
                HashMap::from([
                    ("permit_type".to_string(), permit_type.to_string()),
                    ("permit_type_name".to_string(), name),
                ]),
            ));
        }
    }
    for p in &permits {
        let params = || {
            HashMap::from([
                ("permit".to_string(), p.code.clone()),
                ("status".to_string(), p.status.clone()),
            ])
        };
        if p.status == "suspended" || p.status == "revalidation_required" {
            return Err(org_validation_failed(
                "PERMIT_SUSPENDED",
                format!(
                    "Permit {} is suspended: revalidate it before starting or resuming the work.",
                    p.code
                ),
                params(),
            ));
        }
        if p.status != "active" {
            return Err(org_validation_failed(
                "PERMIT_NOT_ACTIVE",
                format!(
                    "Permit {} is '{}': it must be active before the work starts or resumes.",
                    p.code, p.status
                ),
                params(),
            ));
        }
        activation_ready(db, p).await?;
        covers_the_work(&p.code, p.expires_at.as_deref(), end)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, TimeZone, Utc};

    use super::{checked_remaining_hours, expiry_refusal, shown_hours, work_end, Hours, WorkEnd};

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 19, 8, 0, 0).single().expect("time")
    }

    fn h(hours: f64) -> Hours {
        Hours::Value(hours)
    }

    #[test]
    fn the_end_of_the_work_is_the_later_of_the_planned_end_and_the_moment_plus_the_duration() {
        let planned = Utc.with_ymd_and_hms(2026, 10, 19, 16, 0, 0).single().expect("time");
        // Planned end later than the moment plus the duration: the planned end.
        assert_eq!(
            work_end(Some(" 2026-10-19T16:00:00Z "), &h(2.0), None, at()),
            WorkEnd::Planned(planned)
        );
        // Equal: the planned end.
        assert_eq!(
            work_end(Some("2026-10-19T16:00:00Z"), &h(8.0), None, at()),
            WorkEnd::Planned(planned)
        );
        // The moment plus the duration is later: that estimated end.
        assert_eq!(
            work_end(Some("2026-10-19T16:00:00Z"), &h(48.0), None, at()),
            WorkEnd::Estimated {
                end: at() + Duration::hours(48),
                hours: 48.0,
                remaining: false
            }
        );
        assert_eq!(
            work_end(None, &h(2.5), None, at()),
            WorkEnd::Estimated {
                end: at() + Duration::minutes(150),
                hours: 2.5,
                remaining: false
            }
        );
        assert_eq!(
            work_end(Some("  "), &h(1.0), None, at()),
            WorkEnd::Estimated {
                end: at() + Duration::hours(1),
                hours: 1.0,
                remaining: false
            }
        );
        // No planned end and no usable duration: missing, zero, negative or not a finite number.
        for hours in [Hours::Absent, h(0.0), h(-3.0), h(f64::NAN), h(f64::INFINITY)] {
            assert_eq!(work_end(None, &hours, None, at()), WorkEnd::Unknown, "{hours:?}");
        }
        // A duration beyond the calendar ends after any expiry.
        assert!(matches!(
            work_end(None, &h(1e300), None, at()),
            WorkEnd::Estimated { end, .. } if end == DateTime::<Utc>::MAX_UTC
        ));
        for bad in ["someday", "2026-10-19", "19/10/2026 16:00"] {
            assert_eq!(
                work_end(Some(bad), &h(1.0), None, at()),
                WorkEnd::Unreadable(bad.to_string())
            );
        }
    }

    /// Review SF1: a planned end already past (late start, resume after a long hold, reopening) is
    /// no longer the end judged: the moment plus the duration is; with no duration, refused.
    #[test]
    fn a_planned_end_already_past_gives_way_to_the_moment_plus_the_duration() {
        let yesterday_noon = "2026-10-18T12:00:00Z";
        let past = Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).single().expect("time");
        assert_eq!(
            work_end(Some(yesterday_noon), &h(4.0), None, at()),
            WorkEnd::Estimated {
                end: at() + Duration::hours(4),
                hours: 4.0,
                remaining: false
            }
        );
        for none in [Hours::Absent, h(0.0), h(f64::NAN)] {
            assert_eq!(
                work_end(Some(yesterday_noon), &none, None, at()),
                WorkEnd::PlannedPassed(past),
                "{none:?}"
            );
        }
        // A planned end exactly at the moment has passed too.
        assert_eq!(
            work_end(Some("2026-10-19T08:00:00Z"), &Hours::Absent, None, at()),
            WorkEnd::PlannedPassed(at())
        );
        // The remaining time entered on a resume replaces the duration (and stands in for a
        // missing one).
        for expected in [h(4.0), Hours::Absent, Hours::Unreadable("abc".into())] {
            assert_eq!(
                work_end(Some(yesterday_noon), &expected, Some(1.5), at()),
                WorkEnd::Estimated {
                    end: at() + Duration::minutes(90),
                    hours: 1.5,
                    remaining: true
                },
                "{expected:?}"
            );
        }
        // A remaining time shorter than the time left to a future planned end: the planned end.
        assert_eq!(
            work_end(Some("2026-10-19T16:00:00Z"), &h(10.0), Some(1.0), at()),
            WorkEnd::Planned(Utc.with_ymd_and_hms(2026, 10, 19, 16, 0, 0).single().expect("time"))
        );
        // An unusable remaining time never shortens anything: the expected duration is used.
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                work_end(None, &h(4.0), Some(bad), at()),
                WorkEnd::Estimated {
                    end: at() + Duration::hours(4),
                    hours: 4.0,
                    remaining: false
                }
            );
        }
        // An expected duration that is not a number, without a remaining time: refused.
        assert_eq!(
            work_end(
                Some("2026-10-20T08:00:00Z"),
                &Hours::Unreadable("abc".into()),
                None,
                at()
            ),
            WorkEnd::DurationUnreadable("abc".into())
        );
    }

    #[test]
    fn hours_are_read_the_same_from_text_and_numbers() {
        assert_eq!(Hours::from_text(None), Hours::Absent);
        assert_eq!(Hours::from_text(Some("  ")), Hours::Absent);
        assert_eq!(Hours::from_text(Some(" 4 ")), Hours::Value(4.0));
        assert_eq!(Hours::from_text(Some("2.5")), Hours::Value(2.5));
        assert_eq!(Hours::from_text(Some("four")), Hours::Unreadable("four".into()));
        assert_eq!(Hours::from_text(Some("4,5")), Hours::Unreadable("4,5".into()));
    }

    #[test]
    fn a_remaining_time_must_be_a_number_of_hours_greater_than_zero() {
        assert_eq!(checked_remaining_hours(None).expect("none"), None);
        assert_eq!(checked_remaining_hours(Some(0.25)).expect("ok"), Some(0.25));
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            match checked_remaining_hours(Some(bad)) {
                Err(crate::errors::AppError::OrgValidationFailed(issues)) => {
                    assert_eq!(issues[0].code, "WO_REMAINING_TIME_INVALID", "{bad}");
                }
                other => panic!("expected WO_REMAINING_TIME_INVALID for {bad}, got {other:?}"),
            }
        }
    }

    /// Review NOTE 4: the hours in a refusal are rounded to at most two decimals.
    #[test]
    fn hours_in_a_refusal_are_rounded() {
        assert_eq!(shown_hours(0.1 + 0.2), "0.3");
        assert_eq!(shown_hours(48.0), "48");
        assert_eq!(shown_hours(2.5), "2.5");
        assert_eq!(shown_hours(1.0 / 3.0), "0.33");
        assert_eq!(shown_hours(2.999), "3");
    }

    /// Review N2: the hours are counted in whole seconds rounded up: a tiny time still ends after the
    /// moment, never at or before it.
    #[test]
    fn hours_round_up_to_the_next_second() {
        assert_eq!(
            work_end(None, &h(0.0001), None, at()),
            WorkEnd::Estimated {
                end: at() + Duration::seconds(1),
                hours: 0.0001,
                remaining: false
            }
        );
        assert_eq!(
            work_end(Some("2026-10-18T12:00:00Z"), &Hours::Absent, Some(0.0001), at()),
            WorkEnd::Estimated {
                end: at() + Duration::seconds(1),
                hours: 0.0001,
                remaining: true
            }
        );
    }

    #[test]
    fn a_permit_covers_the_work_until_an_expiry_equal_to_its_end() {
        let end = at() + Duration::hours(2);
        let planned = WorkEnd::Planned(end);
        let estimated = WorkEnd::Estimated {
            end,
            hours: 2.0,
            remaining: false,
        };
        assert_eq!(expiry_refusal(end, &planned), None);
        assert_eq!(expiry_refusal(end, &estimated), None);
        let early = end - Duration::seconds(1);
        assert_eq!(
            expiry_refusal(early, &planned),
            Some("PERMIT_EXPIRES_BEFORE_PLANNED_END")
        );
        assert_eq!(
            expiry_refusal(early, &estimated),
            Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END")
        );
        assert_eq!(expiry_refusal(early, &WorkEnd::Unknown), None);
        assert_eq!(
            expiry_refusal(end, &WorkEnd::Unreadable("x".into())),
            Some("WO_PLANNED_END_UNREADABLE")
        );
        // An end that cannot be known refuses even a permit expiring long after.
        let far = at() + Duration::days(365);
        assert_eq!(
            expiry_refusal(far, &WorkEnd::PlannedPassed(at())),
            Some("WO_PLANNED_END_PASSED")
        );
        assert_eq!(
            expiry_refusal(far, &WorkEnd::DurationUnreadable("x".into())),
            Some("WO_EXPECTED_DURATION_UNREADABLE")
        );
    }
}
