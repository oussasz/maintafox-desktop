//! The weekly maintenance schedule of a site (SAP PM weekly schedule / Maximo weekly work plan):
//! one schedule per site and week, the site's crews as rows.
//!
//! - The site's week starts the day after its weekend (`site_work_weeks`, read through
//!   `personnel::calendar::SiteWeekBook`) and lasts seven days, so weeks follow each other without
//!   gap. Holidays do not move the week; they only decide its working days.
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
    let weekend = book.weekend_on(day).unwrap_or(&[]);
    let start_weekday = week_start_weekday(weekend).ok_or_else(|| {
        org_validation_failed(
            "PLANNING_WEEK_NO_WEEKEND",
            "This site's work calendar has no weekend (or only rest days): set its weekend to define its planning week.",
            HashMap::from([("site_id".to_string(), site.to_string())]),
        )
    })?;
    let weekday = i64::from(day.weekday().number_from_monday());
    let back = (weekday - start_weekday).rem_euclid(7);
    let week_start = day - Duration::days(back);
    let week_end = week_start + Duration::days(6);
    let holidays: BTreeSet<NaiveDate> = site_holidays(db, site, week_start, week_end)
        .await?
        .into_iter()
        .collect();
    let working_days: Vec<NaiveDate> = (0..7)
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
    use super::week_start_weekday;

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
