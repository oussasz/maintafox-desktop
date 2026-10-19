//! Permit gate of work order execution: an OT starts, resumes or is reopened to in-progress only
//! when every live permit linked to it is active and ready to protect the work (not expired, LOTO
//! isolations all applied and verified, none removed) — whether or not the OT is flagged as
//! requiring a permit — and, when it requires one, at least one exists; when a plant rule flagged it
//! at conversion (and the flag was not cleared), at least one is of the rule's permit type. A live
//! permit that expires before the end of the work refuses the start too (owner decision: the work
//! would outlive its protection). The end of the work is the planned end; without one, the moment
//! of the start or resume plus the expected duration; without either, nothing is judged (the
//! "permit expiring" notification still gives the expiry date and time). Work already in progress is
//! only warned (planning and the expiry notification). Planning only warns about permits; this gate
//! is where the rule is hard-enforced. Callers run it inside the transaction that changes the status.

use std::collections::HashMap;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};

use super::queries::{activation_ready, list_live_permits_linked_to_work_order};

pub async fn assert_in_progress_permit_gate(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_permit, planned_end, expected_duration_hours, \
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
    let expected_hours: Option<f64> = row
        .try_get("", "expected_duration_hours")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("expected_duration_hours decode: {e}")))?;
    let required = requires == 1;
    // The work starts or resumes now: the end it must be covered until.
    let end = work_end(planned_end.as_deref(), expected_hours, Utc::now());
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
/// Stable code: the work order has no planned end, and a live permit expires before the start or
/// resume moment plus the work order's expected duration.
pub const CODE_EXPIRES_BEFORE_ESTIMATED_END: &str = "PERMIT_EXPIRES_BEFORE_ESTIMATED_END";
/// Stable code: the work order's planned end is not a readable date and time (re-plan the dates).
pub const CODE_PLANNED_END_UNREADABLE: &str = crate::wo::time::CODE_PLANNED_END_UNREADABLE;

/// The end of the work a live permit must cover, as of the moment the work starts or resumes
/// (owner decision; CMMS Advisor, 2026-10-19).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WorkEnd {
    /// The work order's planned end.
    Planned(DateTime<Utc>),
    /// No planned end: the moment of the start or resume plus the expected duration (hours).
    Estimated { end: DateTime<Utc>, hours: f64 },
    /// Neither a planned end nor an expected duration: not judged.
    Unknown,
    /// A planned end that is not a readable date and time: refused (re-plan the dates), never guessed.
    Unreadable(String),
}

/// The end of the work started or resumed at `at`: the planned end when there is one; otherwise
/// `at` plus the expected duration when it is a positive number of hours; otherwise unknown.
pub(crate) fn work_end(planned_end: Option<&str>, expected_hours: Option<f64>, at: DateTime<Utc>) -> WorkEnd {
    if let Some(raw) = planned_end.map(str::trim).filter(|e| !e.is_empty()) {
        return crate::wo::time::planned_time(raw)
            .map_or_else(|| WorkEnd::Unreadable(raw.to_string()), WorkEnd::Planned);
    }
    match expected_hours.filter(|h| h.is_finite() && *h > 0.0) {
        Some(hours) => {
            // Whole seconds; a duration beyond the calendar ends after any expiry.
            let end = Duration::try_seconds((hours * 3600.0).round() as i64)
                .and_then(|d| at.checked_add_signed(d))
                .unwrap_or(DateTime::<Utc>::MAX_UTC);
            WorkEnd::Estimated { end, hours }
        }
        None => WorkEnd::Unknown,
    }
}

/// The refusal code when a permit expiring at `expiry` does not cover the work until `end`:
/// strictly earlier than the end (an expiry equal to it covers the work).
pub(crate) fn expiry_refusal(expiry: DateTime<Utc>, end: &WorkEnd) -> Option<&'static str> {
    match end {
        WorkEnd::Planned(e) if expiry < *e => Some(CODE_EXPIRES_BEFORE_PLANNED_END),
        WorkEnd::Estimated { end: e, .. } if expiry < *e => Some(CODE_EXPIRES_BEFORE_ESTIMATED_END),
        WorkEnd::Unreadable(_) => Some(CODE_PLANNED_END_UNREADABLE),
        _ => None,
    }
}

/// An instant as stored (UTC, `Z`, whole seconds): the form the screens show in local time.
pub(crate) fn stored(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
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

/// The permit covers the work until its end (see [`work_end`]; the permit's expiry is read as an
/// instant, an unreadable one is refused rather than guessed).
fn covers_the_work(permit_code: &str, expires_at: Option<&str>, end: &WorkEnd) -> AppResult<()> {
    let Some(expiry_raw) = expires_at.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    if let WorkEnd::Unreadable(raw) = end {
        return Err(planned_end_unreadable(raw));
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
        (Some(code), WorkEnd::Estimated { end: e, hours }) => {
            let e = stored(*e);
            let message = format!(
                "Permit {permit_code} expires ({expiry_raw}) before the estimated end of the work ({e}: now + the expected duration of {hours} h; the work order has no planned end): issue a permit that covers the work, or plan the work order's end, before starting or resuming it."
            );
            params.insert("end_at".to_string(), e);
            params.insert("hours".to_string(), hours.to_string());
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

    use super::{expiry_refusal, work_end, WorkEnd};

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 19, 8, 0, 0).single().expect("time")
    }

    #[test]
    fn the_end_of_the_work_is_the_planned_end_else_the_moment_plus_the_duration() {
        let planned = Utc.with_ymd_and_hms(2026, 10, 19, 16, 0, 0).single().expect("time");
        assert_eq!(
            work_end(Some(" 2026-10-19T16:00:00Z "), Some(48.0), at()),
            WorkEnd::Planned(planned)
        );
        assert_eq!(
            work_end(None, Some(2.5), at()),
            WorkEnd::Estimated {
                end: at() + Duration::minutes(150),
                hours: 2.5
            }
        );
        assert_eq!(
            work_end(Some("  "), Some(1.0), at()),
            WorkEnd::Estimated {
                end: at() + Duration::hours(1),
                hours: 1.0
            }
        );
        // No estimate: missing, zero, negative or not a number.
        for hours in [None, Some(0.0), Some(-3.0), Some(f64::NAN), Some(f64::INFINITY)] {
            assert_eq!(work_end(None, hours, at()), WorkEnd::Unknown, "{hours:?}");
        }
        // A duration beyond the calendar ends after any expiry.
        assert!(matches!(
            work_end(None, Some(1e300), at()),
            WorkEnd::Estimated { end, .. } if end == DateTime::<Utc>::MAX_UTC
        ));
        for bad in ["someday", "2026-10-19", "19/10/2026 16:00"] {
            assert_eq!(
                work_end(Some(bad), Some(1.0), at()),
                WorkEnd::Unreadable(bad.to_string())
            );
        }
    }

    #[test]
    fn a_permit_covers_the_work_until_an_expiry_equal_to_its_end() {
        let end = at() + Duration::hours(2);
        let planned = WorkEnd::Planned(end);
        let estimated = WorkEnd::Estimated { end, hours: 2.0 };
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
    }
}
