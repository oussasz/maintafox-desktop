//! The weekly maintenance schedule of a site (SAP PM weekly schedule / Maximo weekly work plan):
//! one schedule per site and week, the site's crews as rows.
//!
//! - The site's week starts the day after its weekend (`site_work_weeks`, read through
//!   `personnel::calendar::SiteWeekBook`) — the weekend in force on that first day — and lasts
//!   seven days, so weeks follow each other without gap. When the site changes its weekend, the
//!   week that straddles the change runs to the first start of the new rule (6 to 13 days): still
//!   no gap and no overlap. Holidays do not move the week; they only decide its working days.
//! - A site whose calendar has no weekend has no planning week: refused with a coded error, never a
//!   guessed start day.

use std::collections::{BTreeSet, HashMap};

use chrono::{Datelike, Duration, NaiveDate};
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppResult};
use crate::personnel::calendar::{site_holidays, SiteWeekBook};

/// The ISO weekday (1 = Monday) a week starts on: the day right after the weekend. With a weekend
/// split in two blocks (rare), the day after the block that ends last in the week.
pub fn week_start_weekday(weekend: &[i64]) -> Option<i64> {
    let days: BTreeSet<i64> = weekend.iter().copied().filter(|d| (1..=7).contains(d)).collect();
    if days.is_empty() || days.len() == 7 {
        return None;
    }
    // Weekdays that follow a weekend day and are not themselves weekend days.
    let starts: Vec<i64> = (1..=7)
        .filter(|d| {
            let previous = if *d == 1 { 7 } else { d - 1 };
            days.contains(&previous) && !days.contains(d)
        })
        .collect();
    starts.last().copied()
}

/// Longest week: a weekend change can stretch the week that straddles it up to 13 days.
const MAX_WEEK_DAYS: i64 = 13;

/// First and last day of the week containing `day`, from the weekend in force on each day
/// (`weekend_on`): the week starts on the nearest day at or before `day` that is the day after
/// the weekend in force on it, and ends the day before the next such day (seven days later while
/// the rule does not change). `None`: the weekend in force on `day` gives no week start.
pub fn week_bounds(
    day: NaiveDate,
    weekend_on: impl Fn(NaiveDate) -> Option<Vec<i64>>,
) -> Option<(NaiveDate, NaiveDate)> {
    week_start_weekday(&weekend_on(day)?)?;
    let is_start = |d: NaiveDate| {
        weekend_on(d)
            .and_then(|w| week_start_weekday(&w))
            .is_some_and(|start| start == i64::from(d.weekday().number_from_monday()))
    };
    let start = (0..=MAX_WEEK_DAYS)
        .map(|back| day - Duration::days(back))
        .find(|d| is_start(*d))?;
    let end = (1..=MAX_WEEK_DAYS)
        .map(|ahead| start + Duration::days(ahead))
        .find(|d| is_start(*d))
        .map_or(start + Duration::days(6), |next| next - Duration::days(1));
    (end >= day).then_some((start, end))
}

/// One week of a site: its days, the working ones and the last working day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteWeek {
    pub entity_id: i64,
    /// First day (the day after the weekend) and last day of the week, `YYYY-MM-DD`.
    pub week_start: NaiveDate,
    pub week_end: NaiveDate,
    /// Days that are neither a weekend day nor a holiday of the site.
    pub working_days: Vec<NaiveDate>,
    /// The last of them (`None`: the whole week is off).
    pub last_working_day: Option<NaiveDate>,
}

/// The week of `site` that contains `day`.
pub async fn site_week(db: &impl ConnectionTrait, site: i64, day: NaiveDate) -> AppResult<SiteWeek> {
    let book = SiteWeekBook::load(db, Some(site)).await?;
    let (week_start, week_end) = week_bounds(day, |d| book.weekend_on(d).map(<[i64]>::to_vec)).ok_or_else(|| {
        org_validation_failed(
            "PLANNING_WEEK_NO_WEEKEND",
            "This site's work calendar has no weekend (or only rest days): set its weekend to define its planning week.",
            HashMap::from([("site_id".to_string(), site.to_string())]),
        )
    })?;
    let holidays: BTreeSet<NaiveDate> = site_holidays(db, site, week_start, week_end)
        .await?
        .into_iter()
        .collect();
    let working_days: Vec<NaiveDate> = (0..=(week_end - week_start).num_days())
        .map(|i| week_start + Duration::days(i))
        .filter(|d| !book.is_rest_day(*d) && !holidays.contains(d))
        .collect();
    Ok(SiteWeek {
        entity_id: site,
        week_start,
        week_end,
        last_working_day: working_days.last().copied(),
        working_days,
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{week_bounds, week_start_weekday};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date")
    }

    #[test]
    fn the_week_follows_the_weekend_in_force_at_its_start() {
        // Friday-Saturday weekend (weeks from Sunday) until 2026-06-10, Saturday-Sunday (weeks from
        // Monday) from then on.
        let weekend =
            |day: NaiveDate| -> Option<Vec<i64>> { Some(if day < d("2026-06-10") { vec![5, 6] } else { vec![6, 7] }) };
        assert_eq!(
            week_bounds(d("2026-06-03"), weekend),
            Some((d("2026-05-31"), d("2026-06-06")))
        );
        // The week that straddles the change runs to the first Monday of the new rule.
        assert_eq!(
            week_bounds(d("2026-06-11"), weekend),
            Some((d("2026-06-07"), d("2026-06-14")))
        );
        assert_eq!(
            week_bounds(d("2026-06-14"), weekend),
            Some((d("2026-06-07"), d("2026-06-14")))
        );
        assert_eq!(
            week_bounds(d("2026-06-15"), weekend),
            Some((d("2026-06-15"), d("2026-06-21")))
        );
        // No weekend in force: no week.
        assert_eq!(week_bounds(d("2026-06-15"), |_| Some(vec![])), None);
        assert_eq!(week_bounds(d("2026-06-15"), |_| None), None);
    }

    #[test]
    fn the_week_starts_the_day_after_the_weekend() {
        assert_eq!(week_start_weekday(&[5, 6]), Some(7), "Friday-Saturday weekend: Sunday");
        assert_eq!(week_start_weekday(&[6, 7]), Some(1), "Saturday-Sunday weekend: Monday");
        assert_eq!(week_start_weekday(&[5]), Some(6), "Friday only: Saturday");
        assert_eq!(week_start_weekday(&[7]), Some(1), "Sunday only: Monday");
        assert_eq!(week_start_weekday(&[]), None, "no weekend: no planning week");
        assert_eq!(week_start_weekday(&[1, 2, 3, 4, 5, 6, 7]), None, "every day off");
    }
}
