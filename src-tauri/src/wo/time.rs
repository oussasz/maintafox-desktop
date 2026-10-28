//! Shared timestamp helpers for WO lifecycle writes/reads.
//!
//! All WO mutation paths should use these helpers to keep UTC/Z formatting
//! consistent across intake, execution, closeout, and attachments.

use chrono::{DateTime, Duration, NaiveDateTime, Offset, SecondsFormat, TimeZone, Timelike, Utc};

use crate::errors::{AppError, AppResult};

/// Canonical "now" in UTC RFC3339 with trailing `Z` and second precision.
pub fn now_utc_z() -> String {
    stamp(now_utc_second())
}

/// "Now" in whole seconds (truncated, as [`now_utc_z`] records it). Read once by a status change
/// that is judged (the permit gate) and recorded, so both use the identical moment.
pub fn now_utc_second() -> DateTime<Utc> {
    let now = Utc::now();
    now - Duration::nanoseconds(i64::from(now.timestamp_subsec_nanos()))
}

/// An instant in the canonical stored form (UTC RFC3339, `Z`, whole seconds).
pub fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
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
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "GATE_WO_TIMESTAMP_INVALID:field={field}: {field} must be a valid RFC3339 UTC timestamp."
            )])
        })
}

/// Stable codes: a planned start / planned end that is not a date and time.
pub const CODE_PLANNED_START_UNREADABLE: &str = "WO_PLANNED_START_UNREADABLE";
pub const CODE_PLANNED_END_UNREADABLE: &str = "WO_PLANNED_END_UNREADABLE";

/// Which bound of the work a planned date and time is: decides how a local time that the clock shows
/// twice (autumn change) is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedBound {
    /// A planned start: the earlier of the two instants (as planning reads it).
    Start,
    /// A planned end: the later of the two instants, so a permit is never judged against an end
    /// earlier than the real one (an end read an hour early would let a permit expiring in that hour
    /// pass).
    End,
}

/// A local wall-clock time in `tz` as an instant. Shown twice (autumn change): the earlier instant
/// for a start, the later one for an end (see [`PlannedBound`]). Skipped by the clock (spring
/// change, e.g. 02:30 when 02:00 jumps to 03:00) — readable, never refused as "unreadable":
/// - an end is read with the offset in force just before the jump (02:30 at UTC+1 = 01:30 UTC),
///   the latest reading of the typed time and the one a browser gives (`new Date("…T02:30")`), so a
///   permit expiring in the skipped hour is never judged against an end read too early;
/// - a start is the instant just after the jump (03:00 local, 01:00 UTC): work cannot begin inside
///   the jump, and a start is never judged against a permit's expiry, so its exact reading is not
///   a safety matter.
pub fn local_instant<Tz: TimeZone>(tz: &Tz, local: NaiveDateTime, bound: PlannedBound) -> Option<DateTime<Utc>> {
    let at = |naive: &NaiveDateTime| tz.from_local_datetime(naive);
    match at(&local) {
        chrono::LocalResult::Single(d) => Some(d.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(a, b) => {
            let (a, b) = (a.with_timezone(&Utc), b.with_timezone(&Utc));
            Some(match bound {
                PlannedBound::Start => a.min(b),
                PlannedBound::End => a.max(b),
            })
        }
        chrono::LocalResult::None => {
            // Gaps are whole minutes and last at most a few hours: the first whole minute after
            // `local` the clock shows again is the instant just after the jump.
            let minute = local.with_second(0)?.with_nanosecond(0)?;
            let after = (1..=24 * 60).find_map(|m| {
                at(&(minute + Duration::minutes(m)))
                    .earliest()
                    .map(|d| d.with_timezone(&Utc))
            })?;
            match bound {
                PlannedBound::Start => Some(after),
                PlannedBound::End => {
                    // The offset in force just before the jump.
                    let before = tz
                        .offset_from_utc_datetime(&(after - Duration::seconds(1)).naive_utc())
                        .fix();
                    Some(Utc.from_utc_datetime(&(local - before)))
                }
            }
        }
    }
}

/// A planned date and time as an instant: RFC 3339 as stored; typed without a zone
/// ([`super::shifts::LOCAL_FORMATS`]), in this computer's time zone, read for its `bound`
/// ([`local_instant`]). A bare day or any other text is not a time of day: `None`.
pub fn planned_time(raw: &str, bound: PlannedBound) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if let Ok(instant) = DateTime::parse_from_rfc3339(raw) {
        return Some(instant.with_timezone(&Utc));
    }
    super::shifts::LOCAL_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(raw, format).ok())
        .and_then(|local| local_instant(&chrono::Local, local, bound))
}

/// A planned date and time (`planned_start` with [`CODE_PLANNED_START_UNREADABLE`], `planned_end`
/// with [`CODE_PLANNED_END_UNREADABLE`]) in its stored form: UTC RFC 3339 with `Z`, whole seconds,
/// so every reader (the permit start gate, planning, another PC) reads the same instant. Accepts an
/// RFC 3339 instant, or a date and time typed without a zone (a `datetime-local` field), read in
/// this computer's time zone ([`planned_time`]: a planned end at the autumn change is the later
/// instant; a time skipped in spring is the instant just after the jump), never as UTC.
/// Blank: `None` (no date). A bare day or any other text is refused (coded, FR/EN on screen).
pub fn stored_planned_time(raw: Option<&str>, code: &str) -> AppResult<Option<String>> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    let bound = if code == CODE_PLANNED_END_UNREADABLE {
        PlannedBound::End
    } else {
        PlannedBound::Start
    };
    match planned_time(raw, bound) {
        Some(at) => Ok(Some(at.to_rfc3339_opts(SecondsFormat::Secs, true))),
        None => Err(crate::errors::org_validation_failed(
            code,
            format!("The planned date ({raw}) is not a readable date and time: re-plan the work order's dates."),
            std::collections::HashMap::from([("value".to_string(), raw.to_string())]),
        )),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};

    use super::{local_instant, PlannedBound};

    /// A zone with a daylight-saving change, like central Europe in 2026: UTC+1, and UTC+2 from
    /// 29 March 01:00 UTC (02:00 local jumps to 03:00) to 25 October 01:00 UTC (03:00 local falls
    /// back to 02:00, so 02:00-03:00 is shown twice).
    #[derive(Debug, Clone, Copy)]
    struct DstZone;

    fn summer_starts() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 29)
            .and_then(|d| d.and_hms_opt(1, 0, 0))
            .expect("date")
    }

    fn summer_ends() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 25)
            .and_then(|d| d.and_hms_opt(1, 0, 0))
            .expect("date")
    }

    fn hours(h: i32) -> FixedOffset {
        FixedOffset::east_opt(h * 3600).expect("offset")
    }

    impl TimeZone for DstZone {
        type Offset = FixedOffset;

        fn from_offset(_: &FixedOffset) -> Self {
            DstZone
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).expect("noon"))
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<FixedOffset> {
            // The offsets whose UTC reading gives back that offset; the earlier instant (larger
            // offset) first.
            let fits: Vec<FixedOffset> = [hours(2), hours(1)]
                .into_iter()
                .filter(|o| self.offset_from_utc_datetime(&(*local - o.fix())) == *o)
                .collect();
            match fits.as_slice() {
                [one] => MappedLocalTime::Single(*one),
                [earlier, later] => MappedLocalTime::Ambiguous(*earlier, *later),
                _ => MappedLocalTime::None,
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).expect("midnight"))
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            if *utc >= summer_starts() && *utc < summer_ends() {
                hours(2)
            } else {
                hours(1)
            }
        }
    }

    fn local(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").expect("local time")
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).expect("instant").with_timezone(&Utc)
    }

    /// Review SF2: a planned end in the hour the clock shows twice is the later instant (never an
    /// hour early, which would let a permit expiring in that hour pass); a start keeps the earlier.
    #[test]
    fn an_end_shown_twice_by_the_clock_is_the_later_instant() {
        let fold = local("2026-10-25T02:30");
        assert_eq!(
            local_instant(&DstZone, fold, PlannedBound::End),
            Some(utc("2026-10-25T01:30:00Z"))
        );
        assert_eq!(
            local_instant(&DstZone, fold, PlannedBound::Start),
            Some(utc("2026-10-25T00:30:00Z"))
        );
        // Outside the change: one reading, the same for both bounds.
        for bound in [PlannedBound::Start, PlannedBound::End] {
            assert_eq!(
                local_instant(&DstZone, local("2026-10-25T04:00"), bound),
                Some(utc("2026-10-25T03:00:00Z"))
            );
            assert_eq!(
                local_instant(&DstZone, local("2026-07-01T12:00"), bound),
                Some(utc("2026-07-01T10:00:00Z"))
            );
        }
    }

    /// Review SF2 and d1884404 SF1: a time skipped by the clock (spring jump 02:00 -> 03:00) is never
    /// refused as unreadable. An end is read with the offset in force before the jump (UTC+1: the
    /// latest reading, as a browser reads it, never early); a start is the instant just after the
    /// jump (03:00 local).
    #[test]
    fn a_time_skipped_by_the_clock_is_read_late_for_an_end_and_after_the_jump_for_a_start() {
        for (typed, end) in [
            ("2026-03-29T02:00", "2026-03-29T01:00:00Z"),
            ("2026-03-29T02:30", "2026-03-29T01:30:00Z"),
            ("2026-03-29T02:59", "2026-03-29T01:59:00Z"),
        ] {
            assert_eq!(
                local_instant(&DstZone, local(typed), PlannedBound::End),
                Some(utc(end)),
                "{typed}"
            );
            assert_eq!(
                local_instant(&DstZone, local(typed), PlannedBound::Start),
                Some(utc("2026-03-29T01:00:00Z")),
                "{typed}"
            );
        }
        for bound in [PlannedBound::Start, PlannedBound::End] {
            assert_eq!(
                local_instant(&DstZone, local("2026-03-29T01:59"), bound),
                Some(utc("2026-03-29T00:59:00Z"))
            );
        }
    }
}
